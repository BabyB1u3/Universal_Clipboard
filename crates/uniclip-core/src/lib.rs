use lru::LruCache;
use std::num::NonZeroUsize;
use std::time::{Duration, Instant};

use uniclip_proto::{ClipboardItem, ClipboardPayload};

/// 计算 Text payload 的稳定 id（blake3 hex
/// TODO: 文本规范化（\r\n 统一为 \n）
pub fn compute_item_id(payload: &ClipboardPayload) -> String {
    match payload {
        ClipboardPayload::Text { text } => {
            let mut hasher = blake3::Hasher::new();
            // 加一个前缀做域分离，避免不同类型碰撞
            hasher.update(b"uniclip:text:v1:");
            hasher.update(text.as_bytes());
            hasher.finalize().to_hex().to_string()
        }
    }
}

/// 最近见过的 item id（带 TTL），用于去重/防回环
pub struct RecentSet {
    ttl: Duration,
    cache: LruCache<String, Instant>,
}

impl RecentSet {
    /// capacity: 最多记多少条 id（比如 2048）
    /// ttl: 多久后忘掉（比如 30 秒/2 分钟）
    pub fn new(capacity: usize, ttl: Duration) -> Self {
        let cap = NonZeroUsize::new(capacity.max(1)).unwrap();
        Self {
            ttl,
            cache: LruCache::new(cap),
        }
    }

    /// true 表示“这是新东西”；false 表示“见过了/过期但又重复”
    pub fn check_and_remember(&mut self, id: &str) -> bool {
        self.evict_expired();

        if self.cache.contains(id) {
            // 续一下时间，让热点留得久一点
            self.cache.put(id.to_string(), Instant::now());
            return false;
        }

        self.cache.put(id.to_string(), Instant::now());
        true
    }

    /// TODO: 将value存为过期时间或者改为跟适合TTL的结构
    pub fn evict_expired(&mut self) {
        let ttl = self.ttl;
        let now = Instant::now();

        // LruCache 没有批量过期 API，简单遍历 key 拷贝一份再删
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

/// 从文本构建 ClipboardItem（daemon 轮询到变化时用）
pub fn make_text_item(from_device_id: &str, created_at_ms: u64, text: String) -> ClipboardItem {
    let payload = ClipboardPayload::Text { text };
    let id = compute_item_id(&payload);

    ClipboardItem {
        id,
        from_device_id: from_device_id.to_string(),
        created_at_ms,
        payload,
    }
}
