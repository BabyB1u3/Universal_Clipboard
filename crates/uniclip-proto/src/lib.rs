use serde::{Deserialize, Serialize};

/// 传输协议版本
pub const PROTOCOL_VERSION: u16 = 1;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeviceInfo {
    pub device_id: String,     //TODO: 换为UUID
    pub device_name: String,   // "Mian's MacBook" / "RaspberryPi" 之类
}

/// 剪贴板 payload
/// TODO: 扩展Imgge, Files, Html, Rtf, ...
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ClipboardPayload {
    Text { text: String },
}

/// 一条剪贴板内容 / 消息体
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClipboardItem {
    /// 内容哈希（blake3 hex），用于去重/防回环
    pub id: String,
    /// 来源设备
    pub from_device_id: String,
    /// 毫秒时间戳（Unix epoch ms）
    pub created_at_ms: u64,
    /// 内容
    pub payload: ClipboardPayload,
}

/// 网络层消息
/// TODO: 扩展握手/配对/心跳
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum WireMessage {
    Hello {
        version: u16,
        device: DeviceInfo,
    },
    ClipboardPush {
        item: ClipboardItem,
    },
}
