use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SessionState {
    Disconnected,
    Connecting,
    Connected,
    Authenticating,
    Authenticated,
    Backoff,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PeerSessionSnapshot {
    pub peer_id: String,
    pub addr: Option<String>,
    pub state: SessionState,
    // 预留：以后可以给 UI 展示“多久后重试”
    pub retry_in_ms: Option<u64>,
}