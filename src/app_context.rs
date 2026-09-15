use serde::Serialize;
use serde_json::Value;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::broadcast;

use crate::chain::Chain;
use crate::core_supervisor::CoreSupervisor;
use crate::lan_share::LanDoor;
use crate::proton::Proton;
use crate::psiphon::Psiphon;
use crate::socks_instance::SocksInstanceManager;
use crate::tor::Tor;
use crate::windscribe::Windscribe;

#[derive(Clone)]
pub struct AppContext {
    pub supervisor: Arc<CoreSupervisor>,
    pub chain: Arc<Chain>,
    pub psiphon: Arc<Psiphon>,
    pub tor: Arc<Tor>,
    pub proton: Arc<Proton>,
    pub windscribe: Arc<Windscribe>,
    pub lan_door: Arc<LanDoor>,
    pub socks_mgr: Arc<SocksInstanceManager>,
    
    pub config_dir: PathBuf,
    pub data_dir: PathBuf,
    pub resource_dir: PathBuf,
    pub log_dir: PathBuf,
    
    pub event_tx: broadcast::Sender<EventPayload>,
}

#[derive(Clone, Serialize)]
pub struct EventPayload {
    pub event: String,
    pub payload: Value,
}

impl AppContext {
    pub fn emit<S: Serialize>(&self, event: &str, payload: &S) -> Result<(), ()> {
        if let Ok(v) = serde_json::to_value(payload) {
            let _ = self.event_tx.send(EventPayload {
                event: event.to_string(),
                payload: v,
            });
        }
        Ok(())
    }

    pub fn path(&self) -> &Self {
        self
    }

    pub fn app_config_dir(&self) -> Result<PathBuf, String> {
        Ok(self.config_dir.clone())
    }

    pub fn app_data_dir(&self) -> Result<PathBuf, String> {
        Ok(self.data_dir.clone())
    }

    pub fn resource_dir(&self) -> Result<PathBuf, String> {
        Ok(self.resource_dir.clone())
    }

    pub fn app_log_dir(&self) -> Option<PathBuf> {
        Some(self.log_dir.clone())
    }

    pub fn chain(&self) -> Arc<Chain> { self.chain.clone() }
    pub fn supervisor(&self) -> Arc<CoreSupervisor> { self.supervisor.clone() }
    pub fn lan_door(&self) -> Arc<LanDoor> { self.lan_door.clone() }
    pub fn psiphon(&self) -> Arc<Psiphon> { self.psiphon.clone() }
    pub fn tor(&self) -> Arc<Tor> { self.tor.clone() }
    pub fn proton(&self) -> Arc<Proton> { self.proton.clone() }
    pub fn windscribe(&self) -> Arc<Windscribe> { self.windscribe.clone() }
    pub fn socks_mgr(&self) -> Arc<SocksInstanceManager> { self.socks_mgr.clone() }
}
