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

/// Default TTL for cached responses: 5 minutes.
const DEFAULT_TTL: Duration = Duration::from_secs(300);

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

/// Remove all expired entries from the cache.
pub fn evict_expired() {
    if let Ok(mut cache) = CACHE.lock() {
        let now = Instant::now();
        cache.retain(|_, entry| entry.expires_at > now);
    }
}
