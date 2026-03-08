use anyhow::Result;
use std::collections::BTreeMap;

use uniclip_core::PairingInfo;

use crate::{init_or_create, AppState, PeerRecord};

pub struct TrustStore {
    app: AppState,
}

impl TrustStore {
    pub fn load(listen_port: u16) -> Result<Self> {
        let app = init_or_create(listen_port)?;
        Ok(Self { app })
    }

    pub fn pairing_info(&self) -> PairingInfo {
        PairingInfo {
            device_id: self.app.config.device_id.clone(),
            device_name: self.app.config.device_name.clone(),
            pubkey_b64: self.app.identity.public_key_b64(),
        }
    }

    pub fn device_id(&self) -> &str {
        &self.app.config.device_id
    }

    pub fn device_name(&self) -> &str {
        &self.app.config.device_name
    }

    pub fn public_key_b64(&self) -> String {
        self.app.identity.public_key_b64()
    }

    pub fn listen_port(&self) -> u16 {
        self.app.config.listen_port
    }

    pub fn trusted_peers(&self) -> &BTreeMap<String, PeerRecord> {
        &self.app.config.trusted_peers
    }

    pub fn list_trusted_peers(&self) -> Vec<(String, crate::PeerRecord)> {
        self.app
            .config
            .trusted_peers
            .iter()
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect()
    }

    pub fn is_trusted(&self, peer_id: &str) -> bool {
        self.app.config.trusted_peers.contains_key(peer_id)
    }

    pub fn get_peer(&self, peer_id: &str) -> Option<&PeerRecord> {
        self.app.config.trusted_peers.get(peer_id)
    }

    pub fn add_peer(&mut self, info: PairingInfo, added_at_ms: u64) -> Result<()> {
        self.app.config.trusted_peers.insert(
            info.device_id.clone(),
            PeerRecord {
                device_name: info.device_name,
                pubkey_b64: info.pubkey_b64,
                added_at_ms,
            },
        );
        self.app.save_config()?;
        Ok(())
    }

    pub fn remove_peer(&mut self, peer_id: &str) -> Result<bool> {
        let removed = self.app.config.trusted_peers.remove(peer_id).is_some();
        if removed {
            self.app.save_config()?;
        }
        Ok(removed)
    }

    pub fn config_dir(&self) -> &std::path::Path {
        &self.app.dir
    }
}