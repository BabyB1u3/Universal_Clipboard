use anyhow::{anyhow, Result};
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

use uniclip_core::PairingInfo;
use uniclip_store::TrustStore;

pub struct PairingService {
    store: Arc<Mutex<TrustStore>>,
}

impl PairingService {
    pub fn new(store: Arc<Mutex<TrustStore>>) -> Self {
        Self { store }
    }

    pub fn export_pairing_info(&self) -> Result<PairingInfo> {
        let store = self.store.lock().unwrap();
        Ok(store.pairing_info())
    }

    pub fn export_pairing_json(&self) -> Result<String> {
        let info = self.export_pairing_info()?;
        Ok(serde_json::to_string(&info)?)
    }

    pub fn import_pairing_info(&self, info: PairingInfo) -> Result<()> {
        let mut store = self.store.lock().unwrap();

        let now_ms = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_millis() as u64;

        // 不允许把自己配对进去
        if info.device_id == store.device_id() {
            return Err(anyhow!("cannot pair with self"));
        }

        store.add_peer(info, now_ms)?;
        Ok(())
    }

    pub fn import_pairing_json(&self, s: &str) -> Result<PairingInfo> {
        let info: PairingInfo =
            serde_json::from_str(s).map_err(|e| anyhow!("invalid pairing json: {}", e))?;
        self.import_pairing_info(info.clone())?;
        Ok(info)
    }

    pub fn is_paired(&self, peer_id: &str) -> bool {
        let store = self.store.lock().unwrap();
        store.is_trusted(peer_id)
    }

    pub fn unpair(&self, peer_id: &str) -> Result<bool> {
        let mut store = self.store.lock().unwrap();
        store.remove_peer(peer_id)
    }

    pub fn list_paired_peers(&self) -> Vec<(String, uniclip_store::PeerRecord)> {
        let store = self.store.lock().unwrap();
        store.list_trusted_peers()
    }
}