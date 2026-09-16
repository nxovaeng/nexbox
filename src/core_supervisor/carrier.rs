use std::{
    net::SocketAddr,
    sync::{atomic::Ordering, Arc},
    thread,
    time::{Duration, Instant},
};
use crate::app_context::AppContext;
use crate::carrier::{Carrier, CarrierKind, RunningChain};
use crate::chain::{Chain, ChainRequest, ChainSettings};
use super::types::{CoreProfile, CoreSnapshot, ProxyRoute};
use super::supervisor::{lock, off_thread, CoreSupervisor, SupervisorInner};
use super::logs::{mark_snapshot_dirty, now_millis, supervisor_log};
use super::process::launch;
use super::system_route::{apply_proxy_route, proxy_is_applied, tun_is_possible, tunnel_proxy_route};

const CARRIER_WATCH: Duration = Duration::from_secs(1);
const AETHER_HOP_TIMEOUT: Duration = Duration::from_secs(150);

pub fn start_carrier_blocking(
    app: &AppContext,
    inner: &Arc<SupervisorInner>,
    profile: &CoreProfile,
    generation: u64,
) -> Result<CoreSnapshot, String> {
    {
        let mut snapshot = lock(&inner.snapshot);
        *snapshot = CoreSnapshot {
            state: "connecting".into(),
            transport: Some(profile.carriers.last().proxy_name().into()),
            status_message: Some(format!("Starting {}", profile.carriers.label())),
            started_at: Some(now_millis()),
            ..CoreSnapshot::default()
        };
        mark_snapshot_dirty(inner);
    }
    supervisor_log(
        inner,
        "info",
        format!("carrier {} is starting", profile.carriers.label()),
    );

    let kinds = profile.carriers.kinds();
    let mut hops: Vec<Carrier> = Vec::with_capacity(kinds.len());
    let mut leaving_from: Option<String> = None;

    for (position, kind) in kinds.iter().copied().enumerate() {
        let upstream = hops.last().map(|previous: &Carrier| previous.socks);
        if position > 0 {
            let progress = format!(
                "{} is up; starting {} through it",
                kinds[position - 1].label(),
                kind.label()
            );
            {
                let mut snapshot = lock(&inner.snapshot);
                snapshot.status_message = Some(progress.clone());
                mark_snapshot_dirty(inner);
            }
            supervisor_log(inner, "info", progress);
        }

        let hop = start_hop(app, inner, profile, kind, upstream, generation)?;
        if !inner.is_current(generation) {
            stop_all_hops(app, inner);
            return Err("the connection was stopped while it was starting".into());
        }
        if let Some(region) = hop_exit_region(app, kind) {
            leaving_from = Some(region);
        }
        hops.push(hop);
    }

    let running_chain = match hops.as_slice() {
        [only] => RunningChain::single(*only),
        [first, second] => RunningChain::pair(*first, *second),
        other => return Err(format!("a chain of {} hops is not supported", other.len())),
    };
    let final_listener = running_chain.listener();
    *lock(&inner.chain) = Some(running_chain.clone());

    spawn_carrier_watch(app, inner, generation);

    let snapshot = {
        let mut snapshot = lock(&inner.snapshot);
        snapshot.state = "connected".into();
        snapshot.socks_address = final_listener.to_string();
        snapshot.status_message = leaving_from.map(|region| format!("Exiting in {region}"));
        mark_snapshot_dirty(inner);
        snapshot.clone()
    };
    supervisor_log(
        inner,
        "info",
        format!("connected on {final_listener} through {}", profile.carriers.label()),
    );
    Ok(snapshot)
}

pub fn spawn_carrier_watch(app: &AppContext, inner: &Arc<SupervisorInner>, generation: u64) {
    let app = app.clone();
    let inner = inner.clone();
    thread::spawn(move || loop {
        thread::sleep(CARRIER_WATCH);
        if !inner.is_current(generation) {
            return;
        }
        let kinds = {
            let session = lock(&inner.session);
            match session.as_ref() {
                Some(session) => session.profile.carriers.kinds(),
                None => return,
            }
        };
        let dead = kinds.into_iter().find(|kind| match kind {
            CarrierKind::Psiphon => !app.psiphon().is_alive(),
            CarrierKind::Tor => !app.tor().is_alive(),
            CarrierKind::Aether => !aether_hop_is_alive(&inner),
        });
        let Some(kind) = dead else {
            continue;
        };
        carrier_died(&app, &inner, generation, kind);
        return;
    });
}

pub fn carrier_died(
    app: &AppContext,
    inner: &Arc<SupervisorInner>,
    generation: u64,
    kind: CarrierKind,
) {
    let (holding, chained) = {
        let session = lock(&inner.session);
        let holding = session
            .as_ref()
            .is_some_and(|current| current.profile.kill_switch)
            && proxy_is_applied(inner);
        let chained = session
            .as_ref()
            .filter(|current| current.profile.carriers.is_chained())
            .map(|current| current.profile.carriers.label());
        (holding, chained)
    };
    let what = match &chained {
        Some(label) => format!("{}, carrying {label},", kind.label()),
        None => kind.label().to_string(),
    };

    {
        let mut snapshot = lock(&inner.snapshot);
        if !inner.is_current(generation) {
            return;
        }
        snapshot.state = "error".into();
        snapshot.pid = None;
        snapshot.status_message = None;
        snapshot.blocking = holding;
        snapshot.last_error = Some(if holding {
            format!(
                "{what} stopped. Traffic is being held rather than sent in the clear -- \
                 disconnect to put your system proxy back."
            )
        } else {
            format!("{what} stopped.")
        });
        mark_snapshot_dirty(inner);
    }
    supervisor_log(inner, "error", format!("{what} stopped unexpectedly"));

    app.chain().stop();
    app.lan_door().close();
    stop_all_hops(app, inner);

    inner.generation.fetch_add(1, Ordering::SeqCst);
    *lock(&inner.session) = None;

    if holding {
        supervisor_log(
            inner,
            "warn",
            "the system proxy has been left pointing at the stopped carrier, so traffic fails \
             instead of leaving in the clear. Disconnect to put it back."
                .into(),
        );
    }
}

pub fn stop_carriers(app: &AppContext) {
    app.psiphon().stop();
    app.tor().stop();
}

pub fn aether_hop_is_alive(inner: &SupervisorInner) -> bool {
    let mut guard = lock(&inner.child);
    match guard.as_mut() {
        Some(child) => matches!(child.try_wait(), Ok(None)),
        None => false,
    }
}

pub fn stop_all_hops(app: &AppContext, inner: &SupervisorInner) {
    stop_carriers(app);
    let mut child = lock(&inner.child).take();
    if let Some(child) = child.as_mut() {
        let _ = child.kill();
        let _ = child.wait();
    }
}

pub fn start_hop(
    app: &AppContext,
    inner: &Arc<SupervisorInner>,
    profile: &CoreProfile,
    kind: CarrierKind,
    upstream: Option<SocketAddr>,
    generation: u64,
) -> Result<Carrier, String> {
    match kind {
        CarrierKind::Psiphon => {
            let psiphon = app.psiphon();
            let is_chained = profile.carriers.is_chained() || upstream.is_some();
            psiphon.start(app, &profile.psiphon, upstream, is_chained)?;
            psiphon
                .carrier()
                .ok_or_else(|| "Psiphon reported a listener and then stopped carrying".into())
        }
        CarrierKind::Tor => {
            let tor = app.tor();
            tor.start(app, &profile.tor, upstream)?;
            tor.carrier()
                .ok_or_else(|| "Tor reported a listener and then stopped carrying".into())
        }
        CarrierKind::Aether => start_aether_hop(app, inner, profile, upstream, generation),
    }
}

pub fn start_aether_hop(
    app: &AppContext,
    inner: &Arc<SupervisorInner>,
    profile: &CoreProfile,
    upstream: Option<SocketAddr>,
    generation: u64,
) -> Result<Carrier, String> {
    let mut hop = profile.clone();
    if let Some(address) = upstream {
        if super::types::non_empty(Some(profile.upstream_proxy.as_str())).is_some() {
            return Err(
                "this profile already dials through a proxy of your own, so Aether cannot also \
                 be chained behind another carrier. Clear \"Dial through a local proxy\" under \
                 Routes and transports, or put Aether first in the chain."
                    .into(),
            );
        }
        hop.upstream_proxy = format!("socks5://{address}");
        if !aether_identity_exists(app) {
            return Err(
                "Aether has not registered a device yet, and it cannot register from inside \
                 another carrier. Connect with Aether on its own once, then chain it."
                    .into(),
            );
        }
    }

    launch(app, inner, &hop, 0, generation, false)?;

    let deadline = Instant::now() + AETHER_HOP_TIMEOUT;
    while Instant::now() < deadline {
        if !inner.is_current(generation) {
            return Err("the connection was stopped while it was starting".into());
        }
        if let Some(carrier) = aether_carrier(inner) {
            return Ok(carrier);
        }
        if lock(&inner.child).is_none() {
            let reported = lock(&inner.snapshot).last_error.clone();
            return Err(reported.unwrap_or_else(|| "Aether stopped before it connected".into()));
        }
        thread::sleep(Duration::from_millis(250));
    }
    Err(format!(
        "Aether did not connect in {}s",
        AETHER_HOP_TIMEOUT.as_secs()
    ))
}

pub fn aether_identity_exists(app: &AppContext) -> bool {
    let Ok(data_dir) = app.path().app_data_dir() else {
        return false;
    };
    if data_dir.join("aether").join("aether.toml").is_file() {
        return true;
    }
    let identity = data_dir.join("identity");
    std::fs::read_dir(identity).is_ok_and(|entries| {
        entries.flatten().any(|entry| {
            entry
                .file_name()
                .to_str()
                .is_some_and(|name| name.starts_with("aether") && name.ends_with(".toml"))
        })
    })
}

pub fn hop_exit_region(app: &AppContext, kind: CarrierKind) -> Option<String> {
    match kind {
        CarrierKind::Psiphon => app.psiphon().snapshot().exit_region,
        CarrierKind::Tor | CarrierKind::Aether => None,
    }
}

pub fn current_chain(app: &AppContext, inner: &SupervisorInner) -> Option<RunningChain> {
    if let Some(chain) = lock(&inner.chain).clone() {
        return Some(chain);
    }
    current_carrier(app, inner).map(RunningChain::single)
}

pub fn aether_carrier(inner: &SupervisorInner) -> Option<Carrier> {
    let snapshot = lock(&inner.snapshot);
    if snapshot.state != "connected" {
        return None;
    }
    let Ok(socks) = snapshot.socks_address.parse() else {
        let address = snapshot.socks_address.clone();
        drop(snapshot);
        supervisor_log(
            inner,
            "error",
            format!(
                "the tunnel reports itself connected on {address}, which is not an address \
                 anything can be pointed at; treating it as not carrying traffic"
            ),
        );
        return None;
    };
    Some(Carrier {
        kind: CarrierKind::Aether,
        socks,
        endpoint: snapshot
            .endpoint
            .as_deref()
            .and_then(|value| value.parse::<SocketAddr>().ok())
            .map(|address| address.ip()),
        carries_quic: !matches!(
            snapshot.transport.as_deref(),
            Some("masque-h2") | Some("masque-h3")
        ),
    })
}

pub fn current_carrier(app: &AppContext, inner: &SupervisorInner) -> Option<Carrier> {
    let kind = lock(&inner.session)
        .as_ref()
        .map_or(CarrierKind::Aether, |session| session.profile.carriers.last());
    match kind {
        CarrierKind::Psiphon => return app.psiphon().carrier(),
        CarrierKind::Tor => return app.tor().carrier(),
        CarrierKind::Aether => {}
    }

    if lock(&inner.chain).is_some() {
        return aether_carrier(inner);
    }
    let snapshot = lock(&inner.snapshot);
    if snapshot.state != "connected" {
        return None;
    }
    let Ok(socks) = snapshot.socks_address.parse() else {
        return None;
    };
    Some(Carrier {
        kind: CarrierKind::Aether,
        socks,
        endpoint: snapshot
            .endpoint
            .as_deref()
            .and_then(|value| value.parse::<SocketAddr>().ok())
            .map(|address| address.ip()),
        carries_quic: !matches!(
            snapshot.transport.as_deref(),
            Some("masque-h2") | Some("masque-h3")
        ),
    })
}

pub fn engine_is_wanted(chain_enabled: bool, tun: bool, carriers: Option<&RunningChain>) -> bool {
    chain_enabled || tun || carriers.is_some_and(RunningChain::needs_routing_engine)
}

pub async fn set_chain(
    app: AppContext,
    supervisor: Arc<CoreSupervisor>,
    chain: Arc<Chain>,
    settings: ChainSettings,
) -> Result<bool, String> {
    let inner = &supervisor.inner;

    let (bypass_direct_routes, full_tunnel) = {
        let mut guard = lock(&inner.session);
        match guard.as_mut() {
            Some(session) => {
                session.profile.chain = settings.clone();
                (session.profile.bypass_direct_routes, session.profile.full_tunnel)
            }
            None => (false, false),
        }
    };

    let socks = supervisor.connected_socks();
    let carriers = supervisor.chain(&app);
    if carriers.is_none() && (!settings.enabled || settings.through_tunnel) {
        chain.stop();
        return Ok(false);
    }

    let tun = tun_is_possible(inner, full_tunnel);
    let started = if engine_is_wanted(settings.enabled, tun, carriers.as_ref()) {
        let address = chain.start(
            &app,
            &ChainRequest {
                carriers: carriers.clone(),
                settings: &settings,
                bypass_direct_routes,
                tun,
            },
        )?;
        supervisor_log(
            inner,
            "info",
            format!("chain listening on {address}; every node dials through the tunnel"),
        );
        Some(address)
    } else {
        chain.stop();
        None
    };

    if let Some(listener) = started.or_else(|| carriers.as_ref().map(RunningChain::listener)) {
        app.lan_door().retarget(listener);
    }

    let session = lock(&inner.session);
    if session
        .as_ref()
        .is_some_and(|session| session.profile.system_proxy)
    {
        match (started, socks.as_deref()) {
            (Some(address), _) => apply_proxy_route(&app, inner, ProxyRoute::Chain(address)),
            (None, Some(address)) => {
                if let Some(route) = tunnel_proxy_route(address, inner) {
                    apply_proxy_route(&app, inner, route);
                }
            }
            (None, None) => {}
        }
    }
    Ok(started.is_some())
}

pub async fn set_psiphon_region(
    app: AppContext,
    supervisor: Arc<CoreSupervisor>,
    region: String,
) -> Result<CoreSnapshot, String> {
    let inner = supervisor.inner.clone();
    off_thread("changing the exit country", move || {
        set_psiphon_region_blocking(&app, &inner, region)
    })
    .await
}

pub fn set_psiphon_region_blocking(
    app: &AppContext,
    inner: &Arc<SupervisorInner>,
    region: String,
) -> Result<CoreSnapshot, String> {
    let mut settings = {
        let guard = lock(&inner.session);
        guard.as_ref().map(|s| s.profile.psiphon.clone()).unwrap_or_default()
    };
    settings.egress_region = region.trim().to_string();
    settings.validate()?;

    let profile = {
        let mut guard = lock(&inner.session);
        match guard.as_mut() {
            Some(session) => {
                session.profile.psiphon = settings.clone();
                Some(session.profile.clone())
            }
            None => None,
        }
    };

    let Some(profile) = profile else {
        return Ok(lock(&inner.snapshot).clone());
    };
    if !profile.carriers.contains(CarrierKind::Psiphon) {
        return Err(format!(
            "the exit country is a Psiphon setting, and this connection is using {}",
            profile.carriers.label()
        ));
    }

    let _generation = inner.generation.load(Ordering::SeqCst);
    {
        let mut snapshot = lock(&inner.snapshot);
        snapshot.state = "reconnecting".into();
        snapshot.status_message = Some(match settings.egress_region.as_str() {
            "" => "Moving to the best available exit".into(),
            region => format!("Moving the exit to {region}"),
        });
        mark_snapshot_dirty(inner);
    }
    supervisor_log(
        inner,
        "info",
        match settings.egress_region.as_str() {
            "" => "moving the exit to the best available country".into(),
            region => format!("moving the exit to {region}"),
        },
    );

    app.chain().stop();
    app.lan_door().close();

    inner.generation.fetch_add(1, Ordering::SeqCst);
    let next = inner.generation.load(Ordering::SeqCst);
    stop_all_hops(app, inner);

    *lock(&inner.session) = Some(super::types::Session {
        generation: next,
        profile: profile.clone(),
        attempt: 0,
    });
    start_carrier_blocking(app, inner, &profile, next)
}
