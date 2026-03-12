use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeviceInfo {
    pub device_id: String,
    pub device_name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PairingInfo {
    pub device_id: String,
    pub device_name: String,
    pub pubkey_b64: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ClipboardPayload {
    Text { text: String },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClipboardItem {
    /// 每次 copy 都唯一：用于网络去重、防重复转发
    pub event_id: String,
    /// 内容哈希（blake3 hex），用于去重/防回环
    pub content_hash: String,
    /// 来源设备
    pub from_device_id: String,
    /// 毫秒时间戳（Unix epoch ms）
    pub created_at_ms: u64,
    /// 内容
    pub payload: ClipboardPayload,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NearbyPeer {
    pub device_id: String,
    pub device_name: String,
    pub addr: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum TrustState {
    Unpaired,
    Paired,
    Blocked,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ConnectionState {
    Offline,
    Discovered,
    Connecting,
    Connected,
    Backoff,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum AuthState {
    Unauthenticated,
    Authenticating,
    Authenticated,
    Failed,
}