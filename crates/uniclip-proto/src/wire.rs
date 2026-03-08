use serde::{Deserialize, Serialize};
use uniclip_core::{ClipboardItem, DeviceInfo};

pub const PROTOCOL_VERSION: u16 = 1;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum WireMessage {
    // session bootstrap
    Hello {
        version: u16,
        device: DeviceInfo,
    },

    // clipboard sync
    ClipboardPush {
        item: ClipboardItem,
    },

    // future:
    // PairRequest { ... }
    // PairAccept { ... }
    // AuthHello { ... }
    // AuthChallenge { ... }
    // AuthResponse { ... }
    // AuthOk { ... }
    // Heartbeat { ... }
}

pub fn encode(msg: &WireMessage) -> Result<Vec<u8>, bincode::Error> {
    bincode::serialize(msg)
}

pub fn decode(bytes: &[u8]) -> Result<WireMessage, bincode::Error> {
    bincode::deserialize(bytes)
}