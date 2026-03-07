use lru::LruCache;
use std::num::NonZeroUsize;
use std::time::{Duration, Instant};

/// 最近见过的 key（用于：event_id 去重 or content_hash 抑制回环）
pub struct RecentSet {
    ttl: Duration,
    cache: LruCache<String, Instant>,
}

impl RecentSet {
    pub fn new(capacity: usize, ttl: Duration) -> Self {
        let cap = NonZeroUsize::new(capacity.max(1)).unwrap();
        Self {
            ttl,
            cache: LruCache::new(cap),
        }
    }

    /// 只检查是否还在 TTL 内，不刷新时间戳
    pub fn contains_fresh(&mut self, key: &str) -> bool {
        self.evict_expired();
        self.cache.contains(key)
    }

    /// 检查并记住：
    /// - 已存在：返回 false
    /// - 不存在：写入并返回 true
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

    fn evict_expired(&mut self) {
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