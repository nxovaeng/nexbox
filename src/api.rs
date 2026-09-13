use axum::{
    extract::{Path, State},
    response::{sse::{Event, Sse}, IntoResponse},
    routing::{get, post},
    Json, Router,
};
use futures_core::stream::Stream;
use serde_json::json;
use std::convert::Infallible;

use crate::app_context::AppContext;
use crate::chain::ChainSettings;
use crate::core_supervisor::CoreProfile;
use crate::lan_share::LanSettings;

// ── Parameterless handlers ──────────────────────────────────────────────

async fn handle_runtime_info(State(_ctx): State<AppContext>) -> impl IntoResponse {
    Json(crate::core_supervisor::runtime_info())
}

async fn handle_stop_core(State(ctx): State<AppContext>) -> impl IntoResponse {
    let _ = crate::core_supervisor::stop_core(ctx.clone(), ctx.supervisor()).await;
    Json(json!({"status": "success"}))
}

async fn handle_core_status(State(ctx): State<AppContext>) -> impl IntoResponse {
    Json(crate::core_supervisor::core_status(ctx.supervisor()))
}

async fn handle_core_logs(State(ctx): State<AppContext>) -> impl IntoResponse {
    Json(crate::core_supervisor::core_logs(ctx.supervisor()))
}

async fn handle_load_profile(State(ctx): State<AppContext>) -> impl IntoResponse {
    match crate::core_supervisor::load_profile(ctx).await {
        Ok(profile) => Json(json!(profile)),
        Err(e) => Json(json!({"error": e})),
    }
}

async fn handle_cancel_scan(State(ctx): State<AppContext>) -> impl IntoResponse {
    Json(crate::scanner::cancel_scan(ctx.supervisor()))
}

async fn handle_probe_latency(State(ctx): State<AppContext>) -> impl IntoResponse {
    match crate::latency::probe_latency(ctx.supervisor()).await {
        Ok(res) => Json(json!(res)),
        Err(_) => Json(json!(null)),
    }
}

async fn handle_speed_test(State(ctx): State<AppContext>) -> impl IntoResponse {
    match crate::latency::speed_test(ctx.supervisor()).await {
        Ok(res) => Json(json!(res)),
        Err(e) => Json(json!({"error": e})),
    }
}

async fn handle_exit_info(State(ctx): State<AppContext>) -> impl IntoResponse {
    match crate::latency::exit_info(ctx.supervisor(), ctx.chain()).await {
        Ok(res) => Json(json!(res)),
        Err(e) => Json(json!({"error": e})),
    }
}

async fn handle_chain_status(State(ctx): State<AppContext>) -> impl IntoResponse {
    let chain = ctx.chain();
    let running = chain.is_running();
    let address = chain.address().map(|v| v.to_string());
    Json(json!({ "running": running, "address": address }))
}

async fn handle_psiphon_status(State(ctx): State<AppContext>) -> impl IntoResponse {
    Json(ctx.psiphon().snapshot())
}

async fn handle_tor_status(State(ctx): State<AppContext>) -> impl IntoResponse {
    Json(ctx.tor().snapshot())
}

async fn handle_carriers_available(State(ctx): State<AppContext>) -> impl IntoResponse {
    let mut available = Vec::new();
    if crate::core_supervisor::is_available(&ctx) {
        available.push("aether".to_string());
    }
    if crate::psiphon::is_available(&ctx) {
        available.push("psiphon".into());
    }
    if crate::tor::is_available(&ctx) {
        available.push("tor".into());
    }
    Json(available)
}

async fn handle_full_tunnel_is_permitted() -> impl IntoResponse {
    Json(false)
}

async fn handle_resuming_full_tunnel() -> impl IntoResponse {
    Json(false)
}

async fn handle_restart_as_administrator() -> impl IntoResponse {
    Json(json!({"error": "not supported in headless mode"}))
}

async fn handle_lan_share_status(State(ctx): State<AppContext>) -> impl IntoResponse {
    Json(crate::core_supervisor::lan_share_status(ctx.lan_door()))
}

async fn handle_chain_nodes(State(ctx): State<AppContext>) -> impl IntoResponse {
    match ctx.chain().nodes(ctx.supervisor().carries_quic(&ctx)) {
        Ok(nodes) => Json(json!(nodes)).into_response(),
        Err(e) => (axum::http::StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": e}))).into_response(),
    }
}

// ── Handlers with JSON payloads ─────────────────────────────────────────

#[derive(serde::Deserialize)]
struct ProbeCorePayload {
    profile: Option<CoreProfile>,
}

async fn handle_probe_core(State(ctx): State<AppContext>, Json(payload): Json<ProbeCorePayload>) -> impl IntoResponse {
    Json(crate::core_supervisor::probe_core(ctx, payload.profile).await)
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct DownloadCorePayload {
    core_name: String,
}

async fn handle_core_inventory(State(ctx): State<AppContext>) -> impl IntoResponse {
    Json(crate::core_supervisor::core_inventory(&ctx))
}

async fn handle_download_core(State(ctx): State<AppContext>, Json(payload): Json<DownloadCorePayload>) -> impl IntoResponse {
    let data_dir = ctx.path().app_data_dir().unwrap_or_else(|_| std::path::PathBuf::from("data"));
    let target_dir = data_dir.join(&payload.core_name);
    let res = match payload.core_name.as_str() {
        "aether" => crate::downloader::download_aether(&target_dir).await.map(|_| ()),
        "mihomo" => crate::downloader::download_mihomo(&target_dir).await.map(|_| ()),
        "tor" => crate::downloader::download_tor(&target_dir).await.map(|_| ()),
        "wireproxy" => crate::downloader::download_wireproxy(&target_dir).await.map(|_| ()),
        "warpscout" => crate::warpscout::install(&target_dir),
        other => Err(format!("Automated download not available for '{other}'. Please upload the binary manually.")),
    };
    match res {
        Ok(_) => Json(json!({ "success": true })).into_response(),
        Err(e) => (
            axum::http::StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "success": false, "error": e })),
        ).into_response(),
    }
}

async fn handle_upload_core(
    State(ctx): State<AppContext>,
    Path(name): Path<String>,
    body: axum::body::Bytes,
) -> impl IntoResponse {
    let data_dir = ctx.path().app_data_dir().unwrap_or_else(|_| std::path::PathBuf::from("data"));
    let raw_name: String = name.chars().filter(|c| c.is_ascii_alphanumeric() || *c == '-' || *c == '_').collect();
    if raw_name.is_empty() {
        return (axum::http::StatusCode::BAD_REQUEST, Json(json!({ "success": false, "error": "Invalid core name" }))).into_response();
    }
    if body.is_empty() {
        return (axum::http::StatusCode::BAD_REQUEST, Json(json!({ "success": false, "error": "Empty upload payload" }))).into_response();
    }

    let lower = raw_name.to_ascii_lowercase();
    let (core_dir, binary_name) = if lower.contains("psiphon") {
        ("psiphon", "psiphon")
    } else if lower.contains("tor") {
        ("tor", "tor")
    } else if lower.contains("mihomo") || lower.contains("clash") {
        ("mihomo", "mihomo")
    } else if lower.contains("wireproxy") {
        ("wireproxy", "wireproxy")
    } else if lower.contains("warpscout") {
        ("warpscout", "warpscout")
    } else if lower.contains("aether") {
        ("aether", "aether")
    } else {
        (raw_name.as_str(), raw_name.as_str())
    };

    let target_dir = data_dir.join(core_dir);

    match crate::downloader::save_uploaded_binary(&target_dir, binary_name, &body) {
        Ok(path) => Json(json!({ "success": true, "path": path.to_string_lossy() })).into_response(),
        Err(e) => (axum::http::StatusCode::INTERNAL_SERVER_ERROR, Json(json!({ "success": false, "error": e }))).into_response(),
    }
}

#[derive(serde::Deserialize)]
struct StartCorePayload {
    profile: CoreProfile,
}

async fn handle_start_core(State(ctx): State<AppContext>, Json(payload): Json<StartCorePayload>) -> impl IntoResponse {
    match crate::core_supervisor::start_core(ctx.clone(), ctx.supervisor(), payload.profile).await {
        Ok(snapshot) => Json(json!(snapshot)).into_response(),
        Err(e) => (axum::http::StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": e}))).into_response(),
    }
}

async fn handle_save_profile(State(ctx): State<AppContext>, Json(payload): Json<StartCorePayload>) -> impl IntoResponse {
    let mut profile = payload.profile;
    if profile.name.is_empty() {
        profile.name = crate::core_supervisor::active_profile_id(&ctx);
    }
    match crate::core_supervisor::save_profile(ctx, profile).await {
        Ok(profile) => Json(json!(profile)),
        Err(e) => Json(json!({"error": e})),
    }
}

#[derive(serde::Deserialize)]
struct SaveReportPayload {
    contents: String,
    filename: String,
}

async fn handle_save_report(State(ctx): State<AppContext>, Json(payload): Json<SaveReportPayload>) -> impl IntoResponse {
    match crate::core_supervisor::save_report(ctx, payload.contents, payload.filename).await {
        Ok(path) => Json(json!(path)).into_response(),
        Err(e) => (axum::http::StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": e}))).into_response(),
    }
}

#[derive(serde::Deserialize)]
struct ScanEndpointsPayload {
    profile: CoreProfile,
    limit: Option<u32>,
}

async fn handle_scan_endpoints(State(ctx): State<AppContext>, Json(payload): Json<ScanEndpointsPayload>) -> impl IntoResponse {
    match crate::scanner::scan_endpoints(ctx.clone(), ctx.supervisor(), payload.profile, payload.limit.unwrap_or(8)).await {
        Ok(outcome) => Json(json!(outcome)),
        Err(e) => Json(json!({"error": e})),
    }
}

#[derive(serde::Deserialize)]
struct TestEndpointPayload {
    profile: CoreProfile,
    endpoint: String,
}

async fn handle_test_endpoint(State(ctx): State<AppContext>, Json(payload): Json<TestEndpointPayload>) -> impl IntoResponse {
    match crate::scanner::test_endpoint(ctx.clone(), ctx.supervisor(), payload.profile, payload.endpoint).await {
        Ok(candidate) => Json(json!(candidate)),
        Err(e) => Json(json!({"error": e})),
    }
}

#[derive(serde::Deserialize)]
struct SetChainPayload {
    settings: ChainSettings,
}

async fn handle_set_chain(State(ctx): State<AppContext>, Json(payload): Json<SetChainPayload>) -> impl IntoResponse {
    match crate::core_supervisor::set_chain(ctx.clone(), ctx.supervisor(), ctx.chain(), payload.settings).await {
        Ok(enabled) => Json(json!(enabled)),
        Err(e) => Json(json!({"error": e})),
    }
}

#[derive(serde::Deserialize)]
struct FetchBridgesPayload {
    country: String,
}

async fn handle_fetch_bridges(State(ctx): State<AppContext>, Json(payload): Json<FetchBridgesPayload>) -> impl IntoResponse {
    match crate::moat::fetch_bridges(&payload.country, ctx.supervisor().carrier(&ctx).map(|c| c.socks)) {
        Ok(bridges) => Json(json!(bridges)),
        Err(e) => Json(json!({"error": format!("{}", e)})),
    }
}

#[derive(serde::Deserialize)]
struct SetPsiphonRegionPayload {
    region: String,
}

async fn handle_set_psiphon_region(State(ctx): State<AppContext>, Json(payload): Json<SetPsiphonRegionPayload>) -> impl IntoResponse {
    match crate::core_supervisor::set_psiphon_region(ctx.clone(), ctx.supervisor(), payload.region).await {
        Ok(snapshot) => Json(json!(snapshot)),
        Err(e) => Json(json!({"error": e})),
    }
}

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct PsiphonInfoResponse {
    installed: bool,
    binary_path: Option<String>,
    server_list_path: Option<String>,
    server_list_count: usize,
    active_signature_key: String,
    default_signature_key: String,
    custom_signature_key: Option<String>,
    listen_port: Option<u16>,
    default_standalone_port: u16,
    listen_address: Option<String>,
    default_standalone_address: String,
    egress_region: String,
    available_regions: Vec<String>,
    propagation_channel_id: String,
    sponsor_id: String,
    snapshot: crate::psiphon::PsiphonSnapshot,
}

async fn handle_psiphon_info(State(ctx): State<AppContext>) -> impl IntoResponse {
    let installed_bin = crate::psiphon::locate(&ctx).ok();
    let server_list_path = crate::psiphon::locate_server_list(&ctx).ok();
    let server_list_count = server_list_path
        .as_ref()
        .map(|p| crate::psiphon::count_server_entries(p))
        .unwrap_or(0);
    let settings = crate::psiphon::load_standalone_settings(&ctx);
    let active_signature_key = crate::psiphon::resolve_signature_public_key(&ctx, &settings);
    let channel_id = crate::psiphon::resolve_propagation_channel_id(&settings);
    let sponsor_id = crate::psiphon::resolve_sponsor_id(&settings);

    let snapshot = ctx.psiphon().snapshot();
    let mut available_regions = snapshot.available_regions.clone();
    if available_regions.is_empty() {
        if let Some(ref path) = server_list_path {
            available_regions = crate::psiphon::extract_regions_from_server_list(std::path::Path::new(path));
        }
    }
    if available_regions.is_empty() {
        available_regions = crate::psiphon::KNOWN_REGIONS.iter().map(|&s| s.to_string()).collect();
    }

    Json(PsiphonInfoResponse {
        installed: installed_bin.is_some(),
        binary_path: installed_bin.map(|p| p.to_string_lossy().to_string()),
        server_list_path: server_list_path.map(|p| p.to_string_lossy().to_string()),
        server_list_count,
        active_signature_key,
        default_signature_key: crate::psiphon::DEFAULT_SIGNATURE_PUBLIC_KEY.to_string(),
        custom_signature_key: settings.signature_public_key.clone(),
        listen_port: settings.listen_port,
        default_standalone_port: crate::psiphon::DEFAULT_STANDALONE_PORT,
        listen_address: settings.listen_address.clone(),
        default_standalone_address: "127.0.0.1".to_string(),
        egress_region: settings.egress_region,
        available_regions,
        propagation_channel_id: channel_id,
        sponsor_id,
        snapshot,
    })
}

async fn handle_upload_psiphon_server_list(
    State(ctx): State<AppContext>,
    body: axum::body::Bytes,
) -> impl IntoResponse {
    if body.is_empty() {
        return (
            axum::http::StatusCode::BAD_REQUEST,
            Json(json!({ "success": false, "error": "Empty upload payload" })),
        )
            .into_response();
    }
    let data_dir = ctx
        .path()
        .app_data_dir()
        .unwrap_or_else(|_| std::path::PathBuf::from("data"));
    let psiphon_dir = data_dir.join("psiphon");
    if let Err(e) = std::fs::create_dir_all(&psiphon_dir) {
        return (
            axum::http::StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "success": false, "error": format!("Cannot create psiphon directory: {e}") })),
        )
            .into_response();
    }
    let target_path = psiphon_dir.join(crate::psiphon::SERVER_LIST_FILENAME);
    if let Err(e) = std::fs::write(&target_path, &body) {
        return (
            axum::http::StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "success": false, "error": format!("Cannot write server entries file: {e}") })),
        )
            .into_response();
    }
    let count = crate::psiphon::count_server_entries(&target_path);
    Json(json!({
        "success": true,
        "path": target_path.to_string_lossy(),
        "count": count
    }))
    .into_response()
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct SavePsiphonConfigPayload {
    signature_public_key: Option<String>,
    listen_port: Option<u16>,
    listen_address: Option<String>,
    egress_region: Option<String>,
    propagation_channel_id: Option<String>,
    sponsor_id: Option<String>,
}

async fn handle_save_psiphon_config(
    State(ctx): State<AppContext>,
    Json(payload): Json<SavePsiphonConfigPayload>,
) -> impl IntoResponse {
    let mut settings = crate::psiphon::load_standalone_settings(&ctx);

    if let Some(key) = payload.signature_public_key {
        let trimmed = key.trim().to_string();
        settings.signature_public_key = if trimmed.is_empty() {
            None
        } else {
            Some(trimmed)
        };
    }
    if let Some(port) = payload.listen_port {
        settings.listen_port = if port == 0 { None } else { Some(port) };
    }
    if let Some(addr) = payload.listen_address {
        let trimmed = addr.trim().to_string();
        settings.listen_address = if trimmed.is_empty() {
            None
        } else {
            Some(trimmed)
        };
    }
    if let Some(region) = payload.egress_region {
        settings.egress_region = region.trim().to_string();
    }
    if let Some(chan) = payload.propagation_channel_id {
        let trimmed = chan.trim().to_string();
        settings.propagation_channel_id = if trimmed.is_empty() {
            None
        } else {
            Some(trimmed)
        };
    }
    if let Some(spon) = payload.sponsor_id {
        let trimmed = spon.trim().to_string();
        settings.sponsor_id = if trimmed.is_empty() {
            None
        } else {
            Some(trimmed)
        };
    }

    if let Err(e) = crate::psiphon::save_standalone_settings(&ctx, &settings) {
        return (
            axum::http::StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"error": e})),
        )
            .into_response();
    }
    Json(json!({ "success": true })).into_response()
}

#[derive(serde::Deserialize, Default)]
#[serde(rename_all = "camelCase")]
struct StartPsiphonPayload {
    egress_region: Option<String>,
    listen_port: Option<u16>,
    listen_address: Option<String>,
}

async fn handle_start_psiphon(
    State(ctx): State<AppContext>,
    payload: Option<Json<StartPsiphonPayload>>,
) -> impl IntoResponse {
    let mut settings = crate::psiphon::load_standalone_settings(&ctx);

    if let Some(Json(pl)) = payload {
        let mut changed = false;
        if let Some(region) = pl.egress_region {
            settings.egress_region = region.trim().to_string();
            changed = true;
        }
        if let Some(port) = pl.listen_port {
            if port > 0 {
                settings.listen_port = Some(port);
                changed = true;
            }
        }
        if let Some(addr) = pl.listen_address {
            let trimmed = addr.trim().to_string();
            settings.listen_address = if trimmed.is_empty() {
                None
            } else {
                Some(trimmed)
            };
            changed = true;
        }
        if changed {
            let _ = crate::psiphon::save_standalone_settings(&ctx, &settings);
        }
    }

    let app_clone = ctx.clone();
    let psiphon = ctx.psiphon();

    match tokio::task::spawn_blocking(move || psiphon.start_standalone(&app_clone, &settings)).await {
        Ok(Ok(addr)) => Json(json!({
            "success": true,
            "address": addr.to_string(),
            "snapshot": ctx.psiphon().snapshot()
        }))
        .into_response(),
        Ok(Err(e)) => (
            axum::http::StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "success": false, "error": e })),
        )
            .into_response(),
        Err(e) => (
            axum::http::StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "success": false, "error": e.to_string() })),
        )
            .into_response(),
    }
}

async fn handle_stop_psiphon(State(ctx): State<AppContext>) -> impl IntoResponse {
    ctx.psiphon().stop();
    Json(json!({ "success": true, "snapshot": ctx.psiphon().snapshot() })).into_response()
}

// ── Proton Handlers ─────────────────────────────────────────────────────────

async fn handle_proton_info(State(ctx): State<AppContext>) -> impl IntoResponse {
    let info = ctx.proton().get_info(&ctx).await;
    Json(info)
}

async fn handle_proton_login_guest(State(ctx): State<AppContext>) -> impl IntoResponse {
    match ctx.proton().login_guest(&ctx).await {
        Ok(session) => (
            axum::http::StatusCode::OK,
            Json(json!({
                "success": true,
                "uid": session.uid,
                "certExpiresAt": session.cert_expires_at,
            })),
        )
            .into_response(),
        Err(e) => (
            axum::http::StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "success": false, "error": e })),
        )
            .into_response(),
    }
}

async fn handle_proton_renew_cert(State(ctx): State<AppContext>) -> impl IntoResponse {
    // ensure_valid_session automatically recovers from downtime, expired sessions, and missing certificates
    match ctx.proton().ensure_valid_session(&ctx).await {
        Ok(session) => (
            axum::http::StatusCode::OK,
            Json(json!({
                "success": true,
                "certExpiresAt": session.cert_expires_at,
            })),
        )
            .into_response(),
        Err(e) => (
            axum::http::StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "success": false, "error": e })),
        )
            .into_response(),
    }
}

async fn handle_proton_refresh_servers(State(ctx): State<AppContext>) -> impl IntoResponse {
    let session = match ctx.proton().ensure_valid_session(&ctx).await {
        Ok(s) => s,
        Err(e) => {
            return (
                axum::http::StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({ "success": false, "error": format!("Session recovery failed: {e}") })),
            )
                .into_response();
        }
    };

    match ctx.proton().refresh_servers(&ctx, &session).await {
        Ok(servers) => (
            axum::http::StatusCode::OK,
            Json(json!({ "success": true, "count": servers.len() })),
        )
            .into_response(),
        Err(e) => (
            axum::http::StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "success": false, "error": e })),
        )
            .into_response(),
    }
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct SaveProtonConfigPayload {
    listen_address: Option<String>,
    listen_port: Option<u16>,
    country: Option<String>,
    server_name: Option<String>,
    auto_failover: Option<bool>,
}

async fn handle_save_proton_config(
    State(ctx): State<AppContext>,
    Json(payload): Json<SaveProtonConfigPayload>,
) -> impl IntoResponse {
    let mut settings = ctx.proton().load_settings(&ctx);

    if let Some(addr) = payload.listen_address {
        let trimmed = addr.trim().to_string();
        settings.listen_address = if trimmed.is_empty() { None } else { Some(trimmed) };
    }
    if let Some(port) = payload.listen_port {
        settings.listen_port = if port == 0 { None } else { Some(port) };
    }
    if let Some(country) = payload.country {
        let trimmed = country.trim().to_uppercase();
        settings.country = if trimmed.is_empty() { None } else { Some(trimmed) };
    }
    if let Some(name) = payload.server_name {
        let trimmed = name.trim().to_string();
        settings.server_name = if trimmed.is_empty() { None } else { Some(trimmed) };
    }
    if let Some(failover) = payload.auto_failover {
        settings.auto_failover = failover;
    }

    if let Err(e) = ctx.proton().save_settings(&ctx, &settings) {
        return (
            axum::http::StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "success": false, "error": e })),
        )
            .into_response();
    }
    Json(json!({ "success": true })).into_response()
}

#[derive(serde::Deserialize, Default)]
#[serde(rename_all = "camelCase")]
struct StartProtonPayload {
    country: Option<String>,
    server_name: Option<String>,
    listen_address: Option<String>,
    listen_port: Option<u16>,
}

async fn handle_start_proton(
    State(ctx): State<AppContext>,
    payload: Option<Json<StartProtonPayload>>,
) -> impl IntoResponse {
    let mut settings = ctx.proton().load_settings(&ctx);

    if let Some(Json(pl)) = payload {
        let mut changed = false;
        if let Some(country) = pl.country {
            let trimmed = country.trim().to_uppercase();
            settings.country = if trimmed.is_empty() { None } else { Some(trimmed) };
            changed = true;
        }
        if let Some(server) = pl.server_name {
            let trimmed = server.trim().to_string();
            settings.server_name = if trimmed.is_empty() { None } else { Some(trimmed) };
            changed = true;
        }
        if let Some(port) = pl.listen_port {
            if port > 0 {
                settings.listen_port = Some(port);
                changed = true;
            }
        }
        if let Some(addr) = pl.listen_address {
            let trimmed = addr.trim().to_string();
            settings.listen_address = if trimmed.is_empty() { None } else { Some(trimmed) };
            changed = true;
        }
        if changed {
            let _ = ctx.proton().save_settings(&ctx, &settings);
        }
    }

    match ctx.proton().start_standalone(&ctx, &settings).await {
        Ok(addr) => Json(json!({ "success": true, "address": addr.to_string() })).into_response(),
        Err(e) => (
            axum::http::StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "success": false, "error": e })),
        )
            .into_response(),
    }
}

async fn handle_stop_proton(State(ctx): State<AppContext>) -> impl IntoResponse {
    ctx.proton().stop().await;
    Json(json!({ "success": true })).into_response()
}

#[derive(serde::Deserialize)]
struct ProtonServersQuery {
    country: Option<String>,
}

async fn handle_proton_servers(
    State(ctx): State<AppContext>,
    axum::extract::Query(query): axum::extract::Query<ProtonServersQuery>,
) -> impl IntoResponse {
    let servers = ctx
        .proton()
        .get_servers(&ctx, query.country.as_deref())
        .await;
    Json(servers)
}

#[derive(serde::Deserialize)]
struct SetFullTunnelPayload {
    #[allow(dead_code)]
    enabled: bool,
}

async fn handle_set_full_tunnel(State(_ctx): State<AppContext>, Json(_payload): Json<SetFullTunnelPayload>) -> impl IntoResponse {
    Json(json!({"error": "Full tunnel is not supported in headless mode"}))
}

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct RoutingRulesInfoResponse {
    direct_count: usize,
    block_count: usize,
    direct_path: Option<String>,
    block_path: Option<String>,
    has_direct: bool,
    has_block: bool,
}

async fn handle_routing_rules_info(State(ctx): State<AppContext>) -> impl IntoResponse {
    let data_dir = ctx.path().app_data_dir().unwrap_or_else(|_| std::path::PathBuf::from("data"));
    let direct_path = crate::routing_rules::direct_list_path(&data_dir);
    let block_path = crate::routing_rules::block_list_path(&data_dir);
    let direct_count = crate::routing_rules::count_direct_rules(&data_dir);
    let block_count = crate::routing_rules::count_block_rules(&data_dir);

    Json(RoutingRulesInfoResponse {
        direct_count,
        block_count,
        direct_path: direct_path.exists().then(|| direct_path.to_string_lossy().to_string()),
        block_path: block_path.exists().then(|| block_path.to_string_lossy().to_string()),
        has_direct: direct_count > 0,
        has_block: block_count > 0,
    })
}

async fn handle_upload_direct_rules(
    State(ctx): State<AppContext>,
    body: axum::body::Bytes,
) -> impl IntoResponse {
    let data_dir = ctx.path().app_data_dir().unwrap_or_else(|_| std::path::PathBuf::from("data"));
    let content = String::from_utf8_lossy(&body);
    match crate::routing_rules::save_direct_list(&data_dir, &content) {
        Ok(count) => Json(json!({
            "success": true,
            "count": count,
            "path": crate::routing_rules::direct_list_path(&data_dir).to_string_lossy()
        }))
        .into_response(),
        Err(e) => (
            axum::http::StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "success": false, "error": e })),
        )
            .into_response(),
    }
}

async fn handle_upload_block_rules(
    State(ctx): State<AppContext>,
    body: axum::body::Bytes,
) -> impl IntoResponse {
    let data_dir = ctx.path().app_data_dir().unwrap_or_else(|_| std::path::PathBuf::from("data"));
    let content = String::from_utf8_lossy(&body);
    match crate::routing_rules::save_block_list(&data_dir, &content) {
        Ok(count) => Json(json!({
            "success": true,
            "count": count,
            "path": crate::routing_rules::block_list_path(&data_dir).to_string_lossy()
        }))
        .into_response(),
        Err(e) => (
            axum::http::StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "success": false, "error": e })),
        )
            .into_response(),
    }
}

async fn handle_upload_routes_file(
    State(ctx): State<AppContext>,
    body: axum::body::Bytes,
) -> impl IntoResponse {
    let data_dir = ctx.path().app_data_dir().unwrap_or_else(|_| std::path::PathBuf::from("data"));
    let content = String::from_utf8_lossy(&body);
    match crate::routing_rules::save_combined_routes(&data_dir, &content) {
        Ok((block_count, direct_count)) => Json(json!({
            "success": true,
            "blockCount": block_count,
            "directCount": direct_count
        }))
        .into_response(),
        Err(e) => (
            axum::http::StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "success": false, "error": e })),
        )
            .into_response(),
    }
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct ClearRoutingPayload {
    rule_type: String,
}

async fn handle_clear_routing_list(
    State(ctx): State<AppContext>,
    Json(payload): Json<ClearRoutingPayload>,
) -> impl IntoResponse {
    let data_dir = ctx.path().app_data_dir().unwrap_or_else(|_| std::path::PathBuf::from("data"));
    match payload.rule_type.as_str() {
        "direct" => {
            let _ = crate::routing_rules::clear_direct_list(&data_dir);
        }
        "block" => {
            let _ = crate::routing_rules::clear_block_list(&data_dir);
        }
        _ => {
            let _ = crate::routing_rules::clear_direct_list(&data_dir);
            let _ = crate::routing_rules::clear_block_list(&data_dir);
        }
    }
    Json(json!({ "success": true })).into_response()
}

#[derive(serde::Deserialize)]
struct SetLanSharePayload {
    settings: LanSettings,
}

async fn handle_set_lan_share(State(ctx): State<AppContext>, Json(payload): Json<SetLanSharePayload>) -> impl IntoResponse {
    match crate::core_supervisor::set_lan_share(ctx.supervisor(), ctx.chain(), ctx.lan_door(), payload.settings) {
        Ok(status) => Json(json!(status)),
        Err(e) => Json(json!({"error": e})),
    }
}

#[derive(serde::Deserialize)]
struct ChainTestPayload {
    source: String,
    node: String,
}

async fn handle_chain_test(State(ctx): State<AppContext>, Json(payload): Json<ChainTestPayload>) -> impl IntoResponse {
    match ctx.chain().test(&payload.source, &payload.node) {
        Ok(delay) => Json(json!(delay)),
        Err(e) => Json(json!({"error": e})),
    }
}

#[derive(serde::Deserialize)]
struct ChainSelectPayload {
    node: String,
}

async fn handle_chain_select(State(ctx): State<AppContext>, Json(payload): Json<ChainSelectPayload>) -> impl IntoResponse {
    match ctx.chain().select(&payload.node) {
        Ok(()) => Json(json!({"status": "success"})),
        Err(e) => Json(json!({"error": e})),
    }
}

#[derive(serde::Deserialize)]
struct SetSystemProxyPayload {
    enabled: bool,
}

async fn handle_set_system_proxy(State(ctx): State<AppContext>, Json(payload): Json<SetSystemProxyPayload>) -> impl IntoResponse {
    match crate::core_supervisor::set_system_proxy(ctx.clone(), ctx.supervisor(), ctx.chain(), payload.enabled) {
        Ok(enabled) => Json(json!(enabled)),
        Err(e) => Json(json!({"error": e})),
    }
}

// ── SSE ─────────────────────────────────────────────────────────────────

async fn handle_events(State(ctx): State<AppContext>) -> Sse<impl Stream<Item = Result<Event, Infallible>>> {
    let mut rx = ctx.event_tx.subscribe();

    let stream = async_stream::stream! {
        while let Ok(payload) = rx.recv().await {
            let event = Event::default()
                .event(payload.event)
                .data(payload.payload.to_string());
            yield Ok(event);
        }
    };

    Sse::new(stream).keep_alive(axum::response::sse::KeepAlive::new())
}

// ── Router builder ──────────────────────────────────────────────────────

#[derive(serde::Deserialize)]
struct SwitchProfilePayload { id: String }

#[derive(serde::Deserialize)]
struct CreateProfilePayload { name: String }

#[derive(serde::Deserialize)]
struct DuplicateProfilePayload { id: String, name: String }

#[derive(serde::Deserialize)]
struct RenameProfilePayload { id: String, name: String }

async fn handle_list_profiles(State(ctx): State<AppContext>) -> impl IntoResponse {
    match crate::core_supervisor::list_profiles(&ctx) {
        Ok(profiles) => Json(json!(profiles)).into_response(),
        Err(e) => (axum::http::StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": e}))).into_response(),
    }
}

async fn handle_switch_profile(State(ctx): State<AppContext>, Json(payload): Json<SwitchProfilePayload>) -> impl IntoResponse {
    match crate::core_supervisor::switch_profile(&ctx, payload.id) {
        Ok(_) => Json(json!({"success": true})).into_response(),
        Err(e) => (axum::http::StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": e}))).into_response(),
    }
}

async fn handle_delete_profile(State(ctx): State<AppContext>, Json(payload): Json<SwitchProfilePayload>) -> impl IntoResponse {
    match crate::core_supervisor::delete_profile(&ctx, &payload.id) {
        Ok(_) => Json(json!({"success": true})).into_response(),
        Err(e) => (axum::http::StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": e}))).into_response(),
    }
}

async fn handle_create_profile(State(ctx): State<AppContext>, Json(payload): Json<CreateProfilePayload>) -> impl IntoResponse {
    match crate::core_supervisor::create_profile(&ctx, &payload.name) {
        Ok(summary) => Json(json!(summary)).into_response(),
        Err(e) => (axum::http::StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": e}))).into_response(),
    }
}

async fn handle_duplicate_profile(State(ctx): State<AppContext>, Json(payload): Json<DuplicateProfilePayload>) -> impl IntoResponse {
    match crate::core_supervisor::duplicate_profile(&ctx, &payload.id, &payload.name) {
        Ok(summary) => Json(json!(summary)).into_response(),
        Err(e) => (axum::http::StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": e}))).into_response(),
    }
}

async fn handle_rename_profile(State(ctx): State<AppContext>, Json(payload): Json<RenameProfilePayload>) -> impl IntoResponse {
    match crate::core_supervisor::rename_profile(&ctx, &payload.id, &payload.name) {
        Ok(summary) => Json(json!(summary)).into_response(),
        Err(e) => (axum::http::StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": e}))).into_response(),
    }
}

async fn handle_active_profile_id(State(ctx): State<AppContext>) -> impl IntoResponse {
    Json(json!({"id": crate::core_supervisor::active_profile_id(&ctx)}))
}

pub fn api_router() -> Router<AppContext> {
    Router::new()
        // Parameterless (GET+POST)
        .route("/api/runtime_info", get(handle_runtime_info).post(handle_runtime_info))
        .route("/api/stop_core", get(handle_stop_core).post(handle_stop_core))
        .route("/api/core_status", get(handle_core_status).post(handle_core_status))
        .route("/api/core_logs", get(handle_core_logs).post(handle_core_logs))
        .route("/api/load_profile", get(handle_load_profile).post(handle_load_profile))
        .route("/api/cancel_scan", get(handle_cancel_scan).post(handle_cancel_scan))
        .route("/api/probe_latency", get(handle_probe_latency).post(handle_probe_latency))
        .route("/api/speed_test", get(handle_speed_test).post(handle_speed_test))
        .route("/api/exit_info", get(handle_exit_info).post(handle_exit_info))
        .route("/api/chain_status", get(handle_chain_status).post(handle_chain_status))
        .route("/api/psiphon_status", get(handle_psiphon_status).post(handle_psiphon_status))
        .route("/api/tor_status", get(handle_tor_status).post(handle_tor_status))
        .route("/api/carriers_available", get(handle_carriers_available).post(handle_carriers_available))
        .route("/api/full_tunnel_is_permitted", get(handle_full_tunnel_is_permitted).post(handle_full_tunnel_is_permitted))
        .route("/api/resuming_full_tunnel", get(handle_resuming_full_tunnel).post(handle_resuming_full_tunnel))
        .route("/api/restart_as_administrator", get(handle_restart_as_administrator).post(handle_restart_as_administrator))
        .route("/api/lan_share_status", get(handle_lan_share_status).post(handle_lan_share_status))
        .route("/api/chain_nodes", get(handle_chain_nodes).post(handle_chain_nodes))
        // POST-only
        .route("/api/probe_core", post(handle_probe_core))
        .route("/api/core_inventory", get(handle_core_inventory))
        .route("/api/download_core", post(handle_download_core))
        .route("/api/upload_core/:name", post(handle_upload_core))
        .route("/api/start_core", post(handle_start_core))
        .route("/api/save_profile", post(handle_save_profile))
        .route("/api/list_profiles", get(handle_list_profiles))
        .route("/api/switch_profile", post(handle_switch_profile))
        .route("/api/delete_profile", post(handle_delete_profile))
        .route("/api/create_profile", post(handle_create_profile))
        .route("/api/duplicate_profile", post(handle_duplicate_profile))
        .route("/api/rename_profile", post(handle_rename_profile))
        .route("/api/active_profile_id", get(handle_active_profile_id))
        .route("/api/save_report", post(handle_save_report))
        .route("/api/scan_endpoints", post(handle_scan_endpoints))
        .route("/api/test_endpoint", post(handle_test_endpoint))
        .route("/api/set_chain", post(handle_set_chain))
        .route("/api/fetch_bridges", post(handle_fetch_bridges))
        .route("/api/set_psiphon_region", post(handle_set_psiphon_region))
        .route("/api/psiphon_info", get(handle_psiphon_info).post(handle_psiphon_info))
        .route("/api/upload_psiphon_server_list", post(handle_upload_psiphon_server_list))
        .route("/api/save_psiphon_config", post(handle_save_psiphon_config))
        .route("/api/start_psiphon", post(handle_start_psiphon))
        .route("/api/stop_psiphon", post(handle_stop_psiphon))
        // Proton
        .route("/api/proton_info", get(handle_proton_info).post(handle_proton_info))
        .route("/api/proton_servers", get(handle_proton_servers))
        .route("/api/proton_login_guest", post(handle_proton_login_guest))
        .route("/api/proton_renew_cert", post(handle_proton_renew_cert))
        .route("/api/proton_refresh_servers", post(handle_proton_refresh_servers))
        .route("/api/save_proton_config", post(handle_save_proton_config))
        .route("/api/start_proton", post(handle_start_proton))
        .route("/api/stop_proton", post(handle_stop_proton))
        .route("/api/set_full_tunnel", post(handle_set_full_tunnel))
        .route("/api/set_lan_share", post(handle_set_lan_share))
        .route("/api/chain_test", post(handle_chain_test))
        .route("/api/chain_select", post(handle_chain_select))
        .route("/api/set_system_proxy", post(handle_set_system_proxy))
        // SSE
        .route("/api/events", get(handle_events))
        // WarpScout
        .route("/api/warpscout_status", get(handle_warpscout_status).post(handle_warpscout_status))
        .route("/api/warpscout_install", post(handle_warpscout_install))
        .route("/api/warpscout_bridge", post(handle_warpscout_bridge_account))
        .route("/api/warpscout_register", post(handle_warpscout_register))
        .route("/api/warpscout_scan", post(handle_warpscout_scan))
        // Routing rules (generic Aether direct & block lists)
        .route("/api/routing_rules_info", get(handle_routing_rules_info))
        .route("/api/upload_direct_rules", post(handle_upload_direct_rules))
        .route("/api/upload_block_rules", post(handle_upload_block_rules))
        .route("/api/upload_routes_file", post(handle_upload_routes_file))
        .route("/api/clear_routing_list", post(handle_clear_routing_list))
        .layer(axum::extract::DefaultBodyLimit::max(250 * 1024 * 1024))
}

// ── WarpScout handlers ───────────────────────────────────────────────────────

fn ws_dirs(ctx: &AppContext) -> Result<(std::path::PathBuf, std::path::PathBuf), String> {
    let data_dir = ctx.path().app_data_dir()?;
    let ws_dir = data_dir.join("warpscout");
    Ok((ws_dir, data_dir))
}

async fn handle_warpscout_status(State(ctx): State<AppContext>) -> impl IntoResponse {
    match ws_dirs(&ctx) {
        Ok((bin_dir, config_dir)) => Json(json!(crate::warpscout::status(&bin_dir, &config_dir))).into_response(),
        Err(e) => (axum::http::StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": e}))).into_response(),
    }
}

async fn handle_warpscout_install(State(ctx): State<AppContext>) -> impl IntoResponse {
    let (bin_dir, config_dir) = match ws_dirs(&ctx) {
        Ok(p) => p,
        Err(e) => return (axum::http::StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": e}))).into_response(),
    };
    match tokio::task::spawn_blocking(move || crate::warpscout::install(&bin_dir).map(|_| crate::warpscout::status(&bin_dir, &config_dir))).await {
        Ok(Ok(s))  => Json(json!(s)).into_response(),
        Ok(Err(e)) => (axum::http::StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": e}))).into_response(),
        Err(e)     => (axum::http::StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": e.to_string()}))).into_response(),
    }
}

/// Use an existing aether identity for warpscout scans.
async fn handle_warpscout_bridge_account(State(ctx): State<AppContext>) -> impl IntoResponse {
    let (bin_dir, data_dir) = match ws_dirs(&ctx) {
        Ok(p) => p,
        Err(e) => return (axum::http::StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": e}))).into_response(),
    };
    
    let _config_dir = match ctx.path().app_config_dir() {
        Ok(d) => d,
        Err(e) => return (axum::http::StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": e}))).into_response(),
    };
    
    match tokio::task::spawn_blocking(move || {
        let identity_dir = data_dir.join("identity");
        crate::warpscout::from_aether_identity(&data_dir, &identity_dir)?;
        Ok::<crate::warpscout::WarpScoutStatus, String>(crate::warpscout::status(&bin_dir, &data_dir))
    }).await {
        Ok(Ok(s))  => Json(json!(s)).into_response(),
        Ok(Err(e)) => (axum::http::StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": e.to_string()}))).into_response(),
        Err(e)     => (axum::http::StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": e.to_string()}))).into_response(),
    }
}

/// Register a brand-new WARP account via warpscout (fallback path).
async fn handle_warpscout_register(State(ctx): State<AppContext>) -> impl IntoResponse {
    let (bin_dir, config_dir) = match ws_dirs(&ctx) {
        Ok(p) => p,
        Err(e) => return (axum::http::StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": e}))).into_response(),
    };
    let ws = crate::warpscout::warpscout_path(&bin_dir);
    if !ws.exists() {
        return (axum::http::StatusCode::BAD_REQUEST, Json(json!({"error": "warpscout is not installed"}))).into_response();
    }
    let account_file = crate::warpscout::account_json_path(&config_dir);
    let data_dir = crate::warpscout::warpscout_data_dir(&config_dir);
    let _ = std::fs::create_dir_all(&data_dir);
    match tokio::task::spawn_blocking(move || {
        let out = std::process::Command::new(&ws)
            .arg("register")
            // Use absolute path — no cwd dependency
            .arg("-a").arg(&account_file)
            .arg("-plain")  // disable TUI so stdout/stderr are plain text
            .output()
            .map_err(|e| format!("cannot run warpscout: {e}"))?;
        if !out.status.success() {
            let stderr = String::from_utf8_lossy(&out.stderr);
            let stdout = String::from_utf8_lossy(&out.stdout);
            return Err(format!("warpscout register failed: {stderr}{stdout}"));
        }
        Ok(crate::warpscout::status(&bin_dir, &config_dir))
    }).await {
        Ok(Ok(s))  => Json(json!(s)).into_response(),
        Ok(Err(e)) => (axum::http::StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": e}))).into_response(),
        Err(e)     => (axum::http::StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": e.to_string()}))).into_response(),
    }
}

async fn handle_warpscout_scan(
    State(ctx): State<AppContext>,
    Json(opts): Json<crate::warpscout::ScanOptions>,
) -> impl IntoResponse {
    let (bin_dir, config_dir) = match ws_dirs(&ctx) {
        Ok(p) => p,
        Err(e) => return (axum::http::StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": e}))).into_response(),
    };
    match tokio::task::spawn_blocking(move || crate::warpscout::scan(&bin_dir, &config_dir, opts)).await {
        Ok(Ok(r))  => Json(json!(r)).into_response(),
        Ok(Err(e)) => (axum::http::StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": e}))).into_response(),
        Err(e)     => (axum::http::StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": e.to_string()}))).into_response(),
    }
}
