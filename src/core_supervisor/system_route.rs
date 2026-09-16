use std::{
    net::SocketAddr,
    sync::{atomic::Ordering, Arc},
    thread,
    time::Duration,
};
use crate::app_context::AppContext;
use crate::chain::Chain;
use crate::lan_share::{LanDoor, LanSettings, LanStatus};
use super::types::{ProxyRoute, Session};
use super::supervisor::{lock, CoreSupervisor, SupervisorInner};
use super::logs::{mark_snapshot_dirty, supervisor_log};
use super::process::{handle_exit, launch};

pub const NEEDS_ADMINISTRATOR: &str = "needs-administrator";
const BAD_ROUTE_MS: f64 = 2_000.0;
const BAD_ROUTE_STREAK: u32 = 3;
const ROUTE_WATCH: Duration = Duration::from_secs(10);

pub fn set_system_proxy(
    app: AppContext,
    supervisor: Arc<CoreSupervisor>,
    chain: Arc<Chain>,
    enabled: bool,
) -> Result<bool, String> {
    let inner = &supervisor.inner;

    if let Some(session) = lock(&inner.session).as_mut() {
        session.profile.system_proxy = enabled;
    }

    if !enabled {
        clear_system_proxy(&app, inner);
        return Ok(false);
    }

    let (state, socks) = {
        let snapshot = lock(&inner.snapshot);
        (snapshot.state.clone(), snapshot.socks_address.clone())
    };
    let chain_requested = chain_is_in_play(
        chain.is_running(),
        lock(&inner.session)
            .as_ref()
            .is_some_and(|session| session.profile.chain.enabled),
    );
    if state != "connected" {
        return Ok(false);
    }
    let tunnel_address = socks.parse::<SocketAddr>().ok();
    if let Some(route) = desired_proxy_route(chain_requested, chain.address(), tunnel_address) {
        apply_proxy_route(&app, inner, route);
    } else if chain_requested {
        supervisor_log(
            inner,
            "info",
            "system proxy is waiting for the requested chain to become ready".into(),
        );
        return Ok(false);
    } else {
        let _ = tunnel_proxy_route(&socks, inner);
    }
    Ok(proxy_is_applied(inner))
}

pub fn set_lan_share(
    supervisor: Arc<CoreSupervisor>,
    chain: Arc<Chain>,
    door: Arc<LanDoor>,
    settings: LanSettings,
) -> Result<LanStatus, String> {
    let inner = &supervisor.inner;

    if let Some(session) = lock(&inner.session).as_mut() {
        session.profile.lan_share = settings.clone();
    }

    if !settings.enabled {
        door.close();
        supervisor_log(inner, "info", "network sharing stopped".into());
        return Ok(LanStatus::stopped());
    }

    let carrier = carrier_address(inner, &chain).ok_or(
        "connect first: there is nothing to share until the tunnel is carrying traffic",
    )?;
    let status = door.open(carrier, &settings)?;
    supervisor_log(
        inner,
        "info",
        match (&status.address, status.open) {
            (Some(address), true) => {
                format!("sharing this network through {address} (anyone on the local network may connect)")
            }
            (Some(address), false) => {
                format!("sharing this network through {address} (username and password required)")
            }
            _ => "network sharing failed to start".into(),
        },
    );
    Ok(status)
}

pub fn lan_share_status(door: Arc<LanDoor>) -> LanStatus {
    door.status()
}

pub fn carrier_address(inner: &SupervisorInner, chain: &Chain) -> Option<SocketAddr> {
    if let Some(chain_socks) = chain.address() {
        return Some(chain_socks);
    }
    let snapshot = lock(&inner.snapshot);
    if snapshot.state != "connected" {
        return None;
    }
    snapshot.socks_address.parse().ok()
}

pub fn tun_is_possible(inner: &SupervisorInner, wanted: bool) -> bool {
    if !wanted {
        return false;
    }
    supervisor_log(
        inner,
        "warn",
        "full tunnel is switched on but this copy cannot create a network device; running without it. Switch Full tunnel on again to be offered a restart."
            .into(),
    );
    false
}

pub fn resuming_full_tunnel() -> bool {
    false
}

pub fn full_tunnel_is_permitted() -> bool {
    false
}

pub fn restart_as_administrator(_app: AppContext, _supervisor: Arc<CoreSupervisor>) -> Result<(), String> {
    Err("Elevation not supported in headless mode".to_string())
}

pub async fn set_full_tunnel(
    _app: AppContext,
    supervisor: Arc<CoreSupervisor>,
    _chain: Arc<Chain>,
    enabled: bool,
) -> Result<bool, String> {
    let inner = &supervisor.inner;

    let settings = {
        let mut guard = lock(&inner.session);
        match guard.as_mut() {
            Some(session) => {
                session.profile.full_tunnel = enabled;
                Some((session.profile.chain.clone(), session.profile.bypass_direct_routes))
            }
            None => None,
        }
    };
    let Some((_chain_settings, _bypass_direct_routes)) = settings else {
        return Ok(false);
    };

    Ok(false)
}

pub fn chain_is_in_play(chain_running: bool, chain_enabled: bool) -> bool {
    chain_running || chain_enabled
}

pub fn desired_proxy_route(
    chain_requested: bool,
    chain_address: Option<SocketAddr>,
    tunnel_address: Option<SocketAddr>,
) -> Option<ProxyRoute> {
    if chain_requested {
        chain_address.map(ProxyRoute::Chain)
    } else {
        tunnel_address.map(ProxyRoute::Tunnel)
    }
}

pub fn proxy_is_applied(inner: &SupervisorInner) -> bool {
    lock(&inner.proxy_route).is_some()
}

pub fn proxy_route_needs_update(current: Option<ProxyRoute>, requested: ProxyRoute) -> bool {
    current != Some(requested)
}

pub fn apply_proxy_route(_app: &AppContext, _inner: &SupervisorInner, _route: ProxyRoute) {
    // stubbed in VPS/headless mode
}

pub fn clear_system_proxy(_app: &AppContext, _inner: &SupervisorInner) {
    // stubbed in VPS/headless mode
}

pub fn tunnel_proxy_route(socks: &str, inner: &SupervisorInner) -> Option<ProxyRoute> {
    match socks.parse::<SocketAddr>() {
        Ok(address) => Some(ProxyRoute::Tunnel(address)),
        Err(error) => {
            supervisor_log(
                inner,
                "error",
                format!("cannot point the system proxy at SOCKS5 listener \"{socks}\": {error}"),
            );
            None
        }
    }
}

pub fn spawn_route_watch(app: AppContext, inner: Arc<SupervisorInner>, generation: u64) {
    thread::spawn(move || {
        let mut streak = 0u32;
        loop {
            thread::sleep(ROUTE_WATCH);
            if !inner.is_current(generation) {
                return;
            }
            let (state, socks) = {
                let snapshot = lock(&inner.snapshot);
                (snapshot.state.clone(), snapshot.socks_address.clone())
            };
            if state != "connected" {
                streak = 0;
                continue;
            }
            let Ok(address) = socks.parse::<SocketAddr>() else {
                return;
            };
            match crate::latency::round_trip_ms(address) {
                Some(ms) if ms < BAD_ROUTE_MS => streak = 0,
                _ => streak += 1,
            }
            if streak < BAD_ROUTE_STREAK {
                continue;
            }
            if !inner.is_current(generation) {
                return;
            }
            rescan_bad_route(&app, &inner, generation);
            return;
        }
    });
}

pub fn rescan_bad_route(app: &AppContext, inner: &Arc<SupervisorInner>, generation: u64) {
    let profile = {
        let mut guard = lock(&inner.session);
        let Some(session) = guard.as_mut() else {
            return;
        };
        if session.generation != generation {
            return;
        }
        session.profile.quick_reconnect = false;
        session.attempt = 0;
        session.profile.clone()
    };

    supervisor_log(
        inner,
        "warn",
        format!(
            "this gateway has been slower than {}ms for {} checks in a row; searching for a better one rather than staying on it",
            BAD_ROUTE_MS as u64, BAD_ROUTE_STREAK
        ),
    );

    let next = inner.generation.fetch_add(1, Ordering::SeqCst) + 1;
    app.chain().stop();
    if let Some(child) = lock(&inner.child).take().as_mut() {
        let _ = child.kill();
        let _ = child.wait();
    }

    *lock(&inner.session) = Some(Session { generation: next, profile: profile.clone(), attempt: 0 });
    {
        let mut snapshot = lock(&inner.snapshot);
        snapshot.state = "reconnecting".into();
        snapshot.pid = None;
        snapshot.status_message = Some("Finding a faster gateway".into());
        mark_snapshot_dirty(inner);
    }
    if let Err(error) = launch(app, inner, &profile, 0, next, true) {
        handle_exit(app, inner, next, error);
    }
}
