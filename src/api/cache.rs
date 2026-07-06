#![cfg(feature = "ssr")]

use std::collections::HashMap;
use std::sync::{LazyLock, Mutex};
use std::time::{Duration, Instant};
use chrono::{DateTime, NaiveDate, Utc};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

struct CacheEntry {
    data: String,
    expires_at: Instant,
    created_at_utc: DateTime<Utc>,
    ttl_secs: u64,
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
        let ttl_secs = DEFAULT_TTL.as_secs();
        cache.insert(
            key,
            CacheEntry {
                data,
                expires_at: Instant::now() + DEFAULT_TTL,
                created_at_utc: Utc::now(),
                ttl_secs,
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

/// Remove all cache entries that belong to a specific user.
/// Called on logout to prevent stale data being served to a new session.
pub fn remove_user_cache(account_id: &str) {
    remove_by_prefix(&format!("{}:", account_id));
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

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CachedWeekMeta {
    pub monday: NaiveDate,
    pub last_refresh_utc: DateTime<Utc>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct CachedWeeksIndex {
    pub weeks: Vec<CachedWeekMeta>,
}

pub fn week_cache_key(account_id: &str, monday: NaiveDate) -> String {
    format!("{}:week_cache:{}", account_id, monday)
}

pub fn cached_weeks_index_key(account_id: &str) -> String {
    format!("{}:cached_weeks", account_id)
}

pub fn get_cached_weeks(account_id: &str) -> CachedWeeksIndex {
    let key = cached_weeks_index_key(account_id);
    match get(&key) {
        Some(raw) => serde_json::from_str::<CachedWeeksIndex>(&raw).unwrap_or_default(),
        None => CachedWeeksIndex::default(),
    }
}

pub fn update_cached_week(account_id: &str, monday: NaiveDate, last_refresh_utc: DateTime<Utc>) {
    let mut idx = get_cached_weeks(account_id);
    if let Some(existing) = idx.weeks.iter_mut().find(|w| w.monday == monday) {
        existing.last_refresh_utc = last_refresh_utc;
    } else {
        idx.weeks.push(CachedWeekMeta {
            monday,
            last_refresh_utc,
        });
    }
    idx.weeks.sort_by_key(|w| w.monday);
    idx.weeks.dedup_by_key(|w| w.monday);
    if let Ok(raw) = serde_json::to_string(&idx) {
        put(cached_weeks_index_key(account_id), raw);
    }
}

pub fn prune_old_week_entries(retention_days: i64) {
    let cutoff = Utc::now().date_naive() - chrono::Duration::days(retention_days.max(1));
    if let Ok(mut cache) = CACHE.lock() {
        let keys = cache.keys().cloned().collect::<Vec<_>>();
        for key in keys {
            if !key.contains(":cached_weeks") {
                continue;
            }
            let Some(account_id) = key.split(':').next() else {
                continue;
            };
            let Some(index_entry) = cache.get(&key) else {
                continue;
            };
            let mut index: CachedWeeksIndex =
                serde_json::from_str(&index_entry.data).unwrap_or_default();
            let old_mondays = index
                .weeks
                .iter()
                .filter(|w| w.monday < cutoff)
                .map(|w| w.monday)
                .collect::<Vec<_>>();
            if old_mondays.is_empty() {
                continue;
            }
            index.weeks.retain(|w| w.monday >= cutoff);
            for monday in old_mondays {
                cache.remove(&week_cache_key(account_id, monday));
            }
            if let Ok(raw) = serde_json::to_string(&index) {
                cache.insert(
                    key.clone(),
                    CacheEntry {
                        data: raw,
                        expires_at: Instant::now() + DEFAULT_TTL,
                        created_at_utc: Utc::now(),
                        ttl_secs: DEFAULT_TTL.as_secs(),
                    },
                );
            }
        }
    }
}

fn classify_cache_kind(key: &str) -> &'static str {
    if key.contains(":week_cache:") {
        "week_cache"
    } else if key.ends_with(":cached_weeks") {
        "cached_weeks"
    } else if key.contains(":jira_search:") {
        "jira_search"
    } else if key.contains(":jira_worklogs:") {
        "jira_worklogs"
    } else if key.contains(":timesheet_data:") {
        "timesheet_data"
    } else {
        "unknown"
    }
}

pub fn snapshot_json() -> Result<Value, String> {
    let cache = CACHE
        .lock()
        .map_err(|_| "cache lock unavailable".to_string())?;
    let now = Instant::now();
    let mut entries = Vec::<Value>::new();
    for (key, entry) in cache.iter() {
        if entry.expires_at <= now {
            continue;
        }
        let expires_at_utc =
            entry.created_at_utc + chrono::Duration::seconds(entry.ttl_secs as i64);
        let parsed_value = serde_json::from_str::<Value>(&entry.data).ok();
        let row = match parsed_value {
            Some(value) => json!({
                "key": key,
                "kind": classify_cache_kind(key),
                "created_at_utc": entry.created_at_utc,
                "expires_at_utc": expires_at_utc,
                "value": value,
            }),
            None => json!({
                "key": key,
                "kind": classify_cache_kind(key),
                "created_at_utc": entry.created_at_utc,
                "expires_at_utc": expires_at_utc,
                "raw_value": entry.data,
            }),
        };
        entries.push(row);
    }
    entries.sort_by(|a, b| {
        a.get("key")
            .and_then(|k| k.as_str())
            .cmp(&b.get("key").and_then(|k| k.as_str()))
    });
    Ok(json!({
        "generated_at_utc": Utc::now(),
        "entry_count": entries.len(),
        "entries": entries,
    }))
}
