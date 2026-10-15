//! 翻译结果的内存缓存。
//!
//! 目的有两个：
//! 1. 避免重复请求（Google 免费接口对连续请求会返回 429）
//! 2. 提升交互体感（同一词反复查询时瞬时返回）
//!
//! 策略：TTL + 容量上限。超容量时淘汰最久未访问的条目。

use std::collections::HashMap;
use std::time::{Duration, Instant};

use super::model::TranslateResult;

/// 缓存键：原文 + 源语言 + 目标语言 + 引擎。
///
/// 带上引擎是必须的：同一个词在 Google 和有道下的结果不同，
/// 不加就会串味。
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct CacheKey {
    pub text: String,
    pub from: String,
    pub to: String,
    pub engine: String,
}

impl CacheKey {
    pub fn new(text: &str, from: &str, to: &str, engine: &str) -> Self {
        Self {
            text: text.to_string(),
            from: from.to_string(),
            to: to.to_string(),
            engine: engine.to_string(),
        }
    }
}

struct Entry {
    result: TranslateResult,
    inserted: Instant,
    last_hit: Instant,
}

pub struct TranslateCache {
    map: HashMap<CacheKey, Entry>,
    ttl: Duration,
    capacity: usize,
}

impl TranslateCache {
    /// `ttl_secs`：条目存活时间；`capacity`：最多缓存多少条。
    pub fn new(ttl_secs: u64, capacity: usize) -> Self {
        Self {
            map: HashMap::new(),
            ttl: Duration::from_secs(ttl_secs),
            capacity,
        }
    }

    pub fn get(&mut self, key: &CacheKey) -> Option<TranslateResult> {
        let now = Instant::now();
        let expired = match self.map.get(key) {
            Some(e) => now.duration_since(e.inserted) > self.ttl,
            None => return None,
        };
        if expired {
            self.map.remove(key);
            return None;
        }
        let entry = self.map.get_mut(key)?;
        entry.last_hit = now;
        let mut result = entry.result.clone();
        result.cached = true;
        result.elapsed_ms = 0;
        Some(result)
    }

    pub fn put(&mut self, key: CacheKey, result: TranslateResult) {
        if self.capacity == 0 {
            return;
        }
        self.evict_expired();
        if !self.map.contains_key(&key) && self.map.len() >= self.capacity {
            self.evict_lru();
        }
        let now = Instant::now();
        self.map.insert(
            key,
            Entry {
                result,
                inserted: now,
                last_hit: now,
            },
        );
    }

    pub fn len(&mut self) -> usize {
        self.evict_expired();
        self.map.len()
    }

    pub fn is_empty(&mut self) -> bool {
        self.len() == 0
    }

    pub fn clear(&mut self) {
        self.map.clear();
    }

    fn evict_expired(&mut self) {
        let now = Instant::now();
        let ttl = self.ttl;
        self.map
            .retain(|_, e| now.duration_since(e.inserted) <= ttl);
    }

    fn evict_lru(&mut self) {
        if let Some(key) = self
            .map
            .iter()
            .min_by_key(|(_, e)| e.last_hit)
            .map(|(k, _)| k.clone())
        {
            self.map.remove(&key);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn entry(text: &str) -> TranslateResult {
        crate::translate::google::parse(
            &serde_json::json!([[[text, "source"]], null, "en"]),
            "source",
            "zh-CN",
        )
        .unwrap()
    }

    fn key(text: &str) -> CacheKey {
        CacheKey::new(text, "en", "zh-CN", "google")
    }

    #[test]
    fn cache_hits_overwrite_and_eviction() {
        let mut c = TranslateCache::new(60, 2);
        let a = key("a");
        let b = key("b");
        let d = key("d");
        c.put(a.clone(), entry("A"));
        c.put(b.clone(), entry("B"));
        c.put(b.clone(), entry("B2"));
        assert_eq!(c.len(), 2);
        assert!(c.get(&a).unwrap().cached);
        assert_eq!(c.get(&b).unwrap().elapsed_ms, 0);
        c.map.get_mut(&a).unwrap().last_hit = Instant::now() - Duration::from_secs(5);
        c.put(d.clone(), entry("D"));
        assert!(c.get(&a).is_none());
        assert_eq!(c.get(&b).unwrap().translation, "B2");
        c.clear();
        assert!(c.is_empty());
        let mut disabled = TranslateCache::new(60, 0);
        disabled.put(d, entry("D"));
        assert!(disabled.is_empty());
    }

    #[test]
    fn different_engines_do_not_share_entries() {
        let mut c = TranslateCache::new(60, 10);
        c.put(
            CacheKey::new("a", "en", "zh-CN", "google"),
            entry("google 结果"),
        );
        assert!(
            c.get(&CacheKey::new("a", "en", "zh-CN", "youdao")).is_none(),
            "换引擎必须重新请求，不能拿到别的引擎的缓存"
        );
        assert!(c.get(&CacheKey::new("a", "en", "zh-CN", "google")).is_some());
    }

    #[test]
    fn expired_entries_are_excluded_from_stats() {
        let mut c = TranslateCache::new(60, 2);
        let k = key("a");
        c.put(k.clone(), entry("A"));
        c.map.get_mut(&k).unwrap().inserted = Instant::now() - Duration::from_secs(61);
        assert_eq!(c.len(), 0);
        assert!(c.get(&k).is_none());
    }
}

impl Default for TranslateCache {
    fn default() -> Self {
        // 24 小时、2000 条。单词查询体积很小，2000 条约几 MB。
        Self::new(24 * 60 * 60, 2000)
    }
}
