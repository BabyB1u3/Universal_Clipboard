use uuid::Uuid;
use crate::{ClipboardItem, ClipboardPayload};

/// 内容哈希：同内容必然相同
pub fn compute_content_hash(payload: &ClipboardPayload) -> String {
    match payload {
        ClipboardPayload::Text { text } => {
            let mut hasher = blake3::Hasher::new();
            hasher.update(b"uniclip:text:v1:");
            hasher.update(text.as_bytes());
            hasher.finalize().to_hex().to_string()
        }
    }
}

/// 事件 id：每次产生新事件都唯一（uuid v4）
pub fn new_event_id() -> String {
    Uuid::new_v4().to_string()
}

/// 从文本构建 ClipboardItem：event_id 唯一，content_hash 稳定
pub fn make_text_item(from_device_id: &str, created_at_ms: u64, text: String) -> ClipboardItem {
    let payload = ClipboardPayload::Text { text };
    let content_hash = compute_content_hash(&payload);
    let event_id = new_event_id();

    ClipboardItem {
        event_id,
        content_hash,
        from_device_id: from_device_id.to_string(),
        created_at_ms,
        payload,
    }
}