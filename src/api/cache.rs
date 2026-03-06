#![cfg(feature = "ssr")]

use std::collections::HashMap;
use std::sync::{LazyLock, Mutex};
use std::time::{Duration, Instant};

struct CacheEntry {
    data: String,
    expires_at: Instant,
}

static CACHE: LazyLock<Mutex<HashMap<String, CacheEntry>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

/// Default TTL for cached responses: 5 days.
const DEFAULT_TTL: Duration = Duration::from_secs(5 * 24 * 3600);

/// Retrieve a cached value by key. Returns `None` if absent or expired.
pub fn get(key: &str) -> Option<String> {
    let cache = CACHE.lock().ok()?;
    if let Some(entry) = cache.get(key) {
        if Instant::now() < entry.expires_at {
            return Some(entry.data.clone());
        }
    }
    None
}

/// Store a value in the cache with the default TTL.
pub fn put(key: String, data: String) {
    if let Ok(mut cache) = CACHE.lock() {
        cache.insert(
            key,
            CacheEntry {
                data,
                expires_at: Instant::now() + DEFAULT_TTL,
            },
        );
    }
}

/// Remove a single cache entry by exact key.
pub fn remove(key: &str) {
    if let Ok(mut cache) = CACHE.lock() {
        cache.remove(key);
    }
}

/// Remove all cache entries whose key starts with the given prefix.
pub fn remove_by_prefix(prefix: &str) {
    if let Ok(mut cache) = CACHE.lock() {
        cache.retain(|k, _| !k.starts_with(prefix));
    }
}

/// Remove all entries from the cache.
pub fn clear_all() {
    if let Ok(mut cache) = CACHE.lock() {
        cache.clear();
    }
}

/// Remove all expired entries from the cache.
pub fn evict_expired() {
    if let Ok(mut cache) = CACHE.lock() {
        let now = Instant::now();
        cache.retain(|_, entry| entry.expires_at > now);
    }
}
