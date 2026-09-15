mod api;
mod app_context;
mod carrier;
mod chain;
mod core_supervisor;
mod downloader;
mod embedded;
mod http_bridge;
mod routing_rules;
mod lan_share;
mod latency;
mod moat;
mod proton;
mod psiphon;
mod scanner;
mod socks_instance;
mod tor;
mod warpscout;
mod windscribe;

use app_context::AppContext;
use chain::Chain;
use core_supervisor::CoreSupervisor;
use lan_share::LanDoor;
use proton::Proton;
use psiphon::Psiphon;
use socks_instance::SocksInstanceManager;
use tor::Tor;
use windscribe::Windscribe;

use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::broadcast;
use tower_http::cors::CorsLayer;

#[tokio::main]
async fn main() {
    // Install ring crypto provider for rustls (required in rustls 0.23)
    let _ = rustls::crypto::ring::default_provider().install_default();

    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let base_dir = if cwd.join("data").is_dir() {
        cwd.clone()
    } else if cwd.join("nextvpn").join("data").is_dir() {
        cwd.join("nextvpn")
    } else if let Ok(exe) = std::env::current_exe() {
        if let Some(parent) = exe.parent() {
            if parent.join("data").is_dir() {
                parent.to_path_buf()
            } else if parent.parent().map(|p| p.join("data").is_dir()).unwrap_or(false) {
                parent.parent().unwrap().to_path_buf()
            } else if parent.parent().map(|p| p.join("nextvpn").join("data").is_dir()).unwrap_or(false) {
                parent.parent().unwrap().join("nextvpn")
            } else {
                cwd.clone()
            }
        } else {
            cwd.clone()
        }
    } else {
        cwd.clone()
    };

    let config_dir = base_dir.join("config");
    let data_dir = base_dir.join("data");
    let resource_dir = base_dir.join("resources");
    let log_dir = base_dir.join("logs");

    let _ = std::fs::create_dir_all(&config_dir);
    let _ = std::fs::create_dir_all(&data_dir);
    let _ = std::fs::create_dir_all(&resource_dir);
    let _ = std::fs::create_dir_all(&log_dir);

    let (event_tx, _) = broadcast::channel(100);

    let ctx = AppContext {
        supervisor: Arc::new(CoreSupervisor::new()),
        chain: Arc::new(Chain::new()),
        psiphon: Arc::new(Psiphon::new()),
        tor: Arc::new(Tor::new()),
        proton: Arc::new(Proton::new()),
        windscribe: Arc::new(Windscribe::new()),
        lan_door: Arc::new(LanDoor::default()),
        socks_mgr: Arc::new(SocksInstanceManager::new()),
        config_dir,
        data_dir,
        resource_dir,
        log_dir,
        event_tx: event_tx.clone(),
    };

    // Load persisted Windscribe accounts and servers
    ctx.windscribe().load_initial_data(&ctx).await;

    // Initialize multi-instance SOCKS5 manager and trigger autostart
    ctx.socks_mgr().init(&ctx).await;
    ctx.socks_mgr().trigger_autostart(ctx.clone());

    // Start the background status pump
    core_supervisor::start_pump(ctx.clone(), &ctx.supervisor());

    // Start background watchdog for Proton (handles cert auto-renewal & recovery after downtime)
    ctx.proton().ensure_watchdog(ctx.clone());

    // Build the combined router: API routes + embedded frontend
    let app = api::api_router()
        .fallback(embedded::static_handler)
        .layer(axum::extract::DefaultBodyLimit::max(250 * 1024 * 1024))
        .layer(CorsLayer::permissive())
        .with_state(ctx.clone());

    let addr = "0.0.0.0:14731";
    let listener = tokio::net::TcpListener::bind(addr).await.unwrap();
    println!("NextVPN running on http://{}", addr);
    println!("  API:  http://{}:{}/api/core_status", "127.0.0.1", 14731);
    println!("  Web:  http://{}:{}/", "127.0.0.1", 14731);

    tokio::select! {
        res = axum::serve(listener, app) => {
            if let Err(e) = res {
                eprintln!("Server error: {e}");
            }
        }
        _ = wait_for_signal() => {
            println!("\nShutting down…");
            // Allow a second Ctrl+C to force-kill immediately
            tokio::spawn(async {
                let _ = tokio::signal::ctrl_c().await;
                std::process::exit(130);
            });
            // Kill any running child processes immediately
            ctx.socks_mgr().shutdown_all();
            ctx.supervisor().shutdown(&ctx);
            ctx.proton().stop().await;
            std::process::exit(0);
        }
    }
}

async fn wait_for_signal() {
    // Wait for Ctrl+C (SIGINT) or SIGTERM
    let ctrl_c = async {
        tokio::signal::ctrl_c()
            .await
            .expect("failed to install Ctrl+C handler");
    };

    #[cfg(unix)]
    let terminate = async {
        if let Ok(mut sig) = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()) {
            sig.recv().await;
        } else {
            std::future::pending::<()>().await;
        }
    };
    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        _ = ctrl_c => {},
        _ = terminate => {},
    }
}
