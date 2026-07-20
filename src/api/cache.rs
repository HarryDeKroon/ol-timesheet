#![cfg(feature = "ssr")]

use chrono::{DateTime, NaiveDate, Utc};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::HashMap;
use std::fs;
use std::sync::{LazyLock, Mutex};
use std::time::{Duration, Instant};

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
const CACHE_PERSIST_FILE: &str = "cache.yaml";

#[derive(Debug, Serialize, Deserialize)]
struct PersistedCacheEntry {
    key: String,
    data: String,
    created_at_utc: DateTime<Utc>,
    ttl_secs: u64,
}

#[derive(Debug, Serialize, Deserialize)]
struct PersistedCacheFile {
    last_update_utc: DateTime<Utc>,
    entries: Vec<PersistedCacheEntry>,
}

fn cache_persist_path() -> std::path::PathBuf {
    crate::auth::app_config_dir().join(CACHE_PERSIST_FILE)
}

fn build_persisted_cache_file(cache: &HashMap<String, CacheEntry>) -> PersistedCacheFile {
    let now = Instant::now();
    let entries = cache
        .iter()
        .filter(|(_, entry)| entry.expires_at > now)
        .map(|(key, entry)| PersistedCacheEntry {
            key: key.clone(),
            data: entry.data.clone(),
            created_at_utc: entry.created_at_utc,
            ttl_secs: entry.ttl_secs.max(1),
        })
        .collect::<Vec<_>>();
    PersistedCacheFile {
        last_update_utc: Utc::now(),
        entries,
    }
}

fn write_persisted_cache_file(file: &PersistedCacheFile) {
    let path = cache_persist_path();
    let raw = match serde_yaml::to_string(file) {
        Ok(raw) => raw,
        Err(err) => {
            log::warn!("[cache] failed to serialize cache yaml: {}", err);
            return;
        }
    };
    if let Err(err) = fs::write(&path, raw) {
        log::warn!(
            "[cache] failed to persist cache yaml {}: {}",
            path.display(),
            err
        );
    }
}

fn persist_cache_snapshot(snapshot: Option<PersistedCacheFile>) {
    if let Some(file) = snapshot {
        write_persisted_cache_file(&file);
    }
}

pub fn load_persisted_cache() -> Option<NaiveDate> {
    let path = cache_persist_path();
    if !path.exists() {
        return None;
    }

    let raw = match fs::read_to_string(&path) {
        Ok(raw) => raw,
        Err(err) => {
            log::warn!(
                "[cache] failed to read persisted cache {}: {}",
                path.display(),
                err
            );
            return None;
        }
    };

    let file = match serde_yaml::from_str::<PersistedCacheFile>(&raw) {
        Ok(file) => file,
        Err(err) => {
            log::warn!(
                "[cache] failed to parse persisted cache yaml {}: {}",
                path.display(),
                err
            );
            return None;
        }
    };

    let now_utc = Utc::now();
    let now_instant = Instant::now();
    let mut loaded = 0usize;

    if let Ok(mut cache) = CACHE.lock() {
        cache.clear();
        for entry in file.entries {
            let expires_at_utc =
                entry.created_at_utc + chrono::Duration::seconds(entry.ttl_secs.max(1) as i64);
            let remaining = (expires_at_utc - now_utc).num_seconds();
            if remaining <= 0 {
                continue;
            }
            cache.insert(
                entry.key,
                CacheEntry {
                    data: entry.data,
                    expires_at: now_instant + Duration::from_secs(remaining as u64),
                    created_at_utc: entry.created_at_utc,
                    ttl_secs: entry.ttl_secs.max(1),
                },
            );
            loaded += 1;
        }
    } else {
        log::warn!("[cache] cache lock unavailable while loading persisted cache");
        return None;
    }

    log::info!(
        "[cache] restored {} cache entr{} from {}",
        loaded,
        if loaded == 1 { "y" } else { "ies" },
        path.display()
    );
    Some(
        file.last_update_utc
            .with_timezone(&chrono::Local)
            .date_naive(),
    )
}

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
    let mut snapshot = None;
    if let Ok(mut cache) = CACHE.lock() {
        let ttl_secs = DEFAULT_TTL.as_secs();
        cache.insert(key, cache_entry(data, ttl_secs));
        snapshot = Some(build_persisted_cache_file(&cache));
    }
    persist_cache_snapshot(snapshot);
}

fn cache_entry(data: String, ttl_secs: u64) -> CacheEntry {
    CacheEntry {
        data,
        expires_at: Instant::now() + Duration::from_secs(ttl_secs.max(1)),
        created_at_utc: Utc::now(),
        ttl_secs: ttl_secs.max(1),
    }
}

/// Remove a single cache entry by exact key.
pub fn remove(key: &str) {
    let mut snapshot = None;
    if let Ok(mut cache) = CACHE.lock() {
        if cache.remove(key).is_some() {
            snapshot = Some(build_persisted_cache_file(&cache));
        }
    }
    persist_cache_snapshot(snapshot);
}

/// Remove all cache entries whose key starts with the given prefix.
pub fn remove_by_prefix(prefix: &str) {
    let mut snapshot = None;
    if let Ok(mut cache) = CACHE.lock() {
        let before = cache.len();
        cache.retain(|k, _| !k.starts_with(prefix));
        if cache.len() != before {
            snapshot = Some(build_persisted_cache_file(&cache));
        }
    }
    persist_cache_snapshot(snapshot);
}

/// Remove all cache entries that belong to a specific user.
/// Called on logout to prevent stale data being served to a new session.
pub fn remove_user_cache(account_id: &str) {
    remove_by_prefix(&format!("{}:", account_id));
}

/// Remove all entries from the cache.
pub fn clear_all() {
    let mut snapshot = None;
    if let Ok(mut cache) = CACHE.lock() {
        if !cache.is_empty() {
            cache.clear();
            snapshot = Some(build_persisted_cache_file(&cache));
        }
    }
    persist_cache_snapshot(snapshot);
}

/// Remove all expired entries from the cache.
pub fn evict_expired() {
    let mut snapshot = None;
    if let Ok(mut cache) = CACHE.lock() {
        let before = cache.len();
        let now = Instant::now();
        cache.retain(|_, entry| entry.expires_at > now);
        if cache.len() != before {
            snapshot = Some(build_persisted_cache_file(&cache));
        }
    }
    persist_cache_snapshot(snapshot);
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

pub fn cached_bitbucket_weeks_index_key(account_id: &str) -> String {
    format!("{}:cached_bitbucket_weeks", account_id)
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

pub fn get_cached_bitbucket_weeks(account_id: &str) -> CachedWeeksIndex {
    let key = cached_bitbucket_weeks_index_key(account_id);
    match get(&key) {
        Some(raw) => serde_json::from_str::<CachedWeeksIndex>(&raw).unwrap_or_default(),
        None => CachedWeeksIndex::default(),
    }
}

pub fn has_cached_bitbucket_week(account_id: &str, monday: NaiveDate) -> bool {
    get_cached_bitbucket_weeks(account_id)
        .weeks
        .iter()
        .any(|w| w.monday == monday)
}

pub fn update_cached_bitbucket_week(
    account_id: &str,
    monday: NaiveDate,
    last_refresh_utc: DateTime<Utc>,
) {
    let mut idx = get_cached_bitbucket_weeks(account_id);
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
        put(cached_bitbucket_weeks_index_key(account_id), raw);
    }
}

pub fn prune_old_week_entries(retention_days: i64) {
    let cutoff = Utc::now().date_naive() - chrono::Duration::days(retention_days.max(1));
    let mut snapshot = None;
    if let Ok(mut cache) = CACHE.lock() {
        let mut changed = false;
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
            changed = true;
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

            let bb_key = cached_bitbucket_weeks_index_key(account_id);
            if let Some(bb_entry) = cache.get(&bb_key) {
                let mut bb_index: CachedWeeksIndex =
                    serde_json::from_str(&bb_entry.data).unwrap_or_default();
                bb_index.weeks.retain(|w| w.monday >= cutoff);
                if let Ok(raw) = serde_json::to_string(&bb_index) {
                    cache.insert(bb_key, cache_entry(raw, DEFAULT_TTL.as_secs()));
                }
            }
        }
        if changed {
            snapshot = Some(build_persisted_cache_file(&cache));
        }
    }
    persist_cache_snapshot(snapshot);
}

fn classify_cache_kind(key: &str) -> &'static str {
    if key.contains(":week_cache:") {
        "week_cache"
    } else if key.ends_with(":cached_weeks") {
        "cached_weeks"
    } else if key.ends_with(":cached_bitbucket_weeks") {
        "cached_bitbucket_weeks"
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

pub fn update_user_entries<F>(account_id: &str, mut updater: F)
where
    F: FnMut(&str, &str) -> Option<String>,
{
    let prefix = format!("{}:", account_id);
    let mut snapshot = None;
    if let Ok(mut cache) = CACHE.lock() {
        let mut changed = false;
        let keys = cache
            .iter()
            .filter(|(key, entry)| key.starts_with(&prefix) && entry.expires_at > Instant::now())
            .map(|(key, _)| key.clone())
            .collect::<Vec<_>>();
        for key in keys {
            let Some(existing) = cache.get(&key) else {
                continue;
            };
            let ttl_secs = existing.ttl_secs;
            let current = existing.data.clone();
            if let Some(updated) = updater(&key, &current) {
                cache.insert(key, cache_entry(updated, ttl_secs));
                changed = true;
            }
        }
        if changed {
            snapshot = Some(build_persisted_cache_file(&cache));
        }
    }
    persist_cache_snapshot(snapshot);
}
