use lru::LruCache;
use std::num::NonZeroUsize;
use std::time::{Duration, Instant};
use uuid::Uuid;

use uniclip_proto::{ClipboardItem, ClipboardPayload};

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

/// 最近见过的 key（用于：event_id 去重 or content_hash 抑制回环）
pub struct RecentSet {
    ttl: Duration,
    cache: LruCache<String, Instant>,
}

impl RecentSet {
    pub fn new(capacity: usize, ttl: Duration) -> Self {
        let cap = NonZeroUsize::new(capacity.max(1)).unwrap();
        Self { ttl, cache: LruCache::new(cap) }
    }

    pub fn check_and_remember(&mut self, key: &str) -> bool {
        self.evict_expired();
        if self.cache.contains(key) {
            self.cache.put(key.to_string(), Instant::now());
            return false;
        }
        self.cache.put(key.to_string(), Instant::now());
        true
    }

    pub fn remember(&mut self, key: &str) {
        self.evict_expired();
        self.cache.put(key.to_string(), Instant::now());
    }

    pub fn evict_expired(&mut self) {
        let ttl = self.ttl;
        let now = Instant::now();
        let keys: Vec<String> = self.cache.iter().map(|(k, _)| k.clone()).collect();
        for k in keys {
            if let Some(t) = self.cache.get(&k).copied() {
                if now.duration_since(t) > ttl {
                    self.cache.pop(&k);
                }
            }
        }
    }
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
