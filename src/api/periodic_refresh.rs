#![cfg(feature = "ssr")]

use crate::api::{bitbucket, cache, jira};
use crate::auth::{self, AuthenticatedSessionSnapshot};
use crate::components::week_navigator::week_monday;
use crate::model::{
    BitbucketActivityUpdate, CellActivity, TimesheetData, TimesheetRefreshDiff, TimesheetWsMessage,
    WorkItem, WorklogEntry, YtdHoursUpdate,
};
use axum::extract::ws::{Message, WebSocket};
use chrono::{Local, NaiveDate};
use futures::{SinkExt, StreamExt};
use std::collections::{HashMap, HashSet};
use std::sync::LazyLock;
use std::time::Duration;
use tokio::sync::{Mutex, Notify, RwLock, mpsc};

const DEFAULT_ACTIVE_REFRESH_MINUTES: u64 = 23;
const DEFAULT_IDLE_REFRESH_MINUTES: u64 = 127;

#[derive(Clone, Debug, Default)]
struct RefreshSnapshot {
    work_items: HashMap<String, WorkItem>,
    ytd_hours: HashMap<String, f64>,
    worklogs: HashMap<String, WorklogEntry>,
    bitbucket_activity: HashMap<String, CellActivity>,
}

impl RefreshSnapshot {
    fn visible_issue_keys(&self) -> HashSet<String> {
        self.work_items.keys().cloned().collect()
    }
}

#[derive(Clone)]
struct TimesheetSessionSender {
    session_id: String,
    tx: mpsc::UnboundedSender<String>,
}

#[derive(Default)]
struct ActiveTimesheetUser {
    sessions: HashMap<String, TimesheetSessionSender>,
    last_snapshot: Option<RefreshSnapshot>,
}

static ACTIVE_TIMESHEET_USERS: LazyLock<RwLock<HashMap<String, ActiveTimesheetUser>>> =
    LazyLock::new(|| RwLock::new(HashMap::new()));
static BITBUCKET_WEEK_BACKFILL_INFLIGHT: LazyLock<Mutex<HashSet<String>>> =
    LazyLock::new(|| Mutex::new(HashSet::new()));

/// Fired when first WebSocket session connects (0 → 1 active sessions). Poll
/// loop uses this to wake from idle sleep and switch cadence immediately.
static SESSION_BECAME_ACTIVE: LazyLock<Notify> = LazyLock::new(Notify::new);

fn active_refresh_minutes() -> u64 {
    std::env::var("PERIODIC_REFRESH_ACTIVE_MINUTES")
        .ok()
        .and_then(|s| s.parse::<u64>().ok())
        .unwrap_or(DEFAULT_ACTIVE_REFRESH_MINUTES)
        .max(1)
}

fn idle_refresh_minutes() -> u64 {
    std::env::var("PERIODIC_REFRESH_IDLE_MINUTES")
        .ok()
        .and_then(|s| s.parse::<u64>().ok())
        .unwrap_or(DEFAULT_IDLE_REFRESH_MINUTES)
        .max(1)
}

fn has_timesheet_activity_for_key(ts: &TimesheetData, issue_key: &str) -> bool {
    ts.worklogs.iter().any(|w| w.issue_key == issue_key)
        || ts
            .bitbucket_activity
            .keys()
            .filter_map(|cell_key| cell_key.split_once(':').map(|(key, _)| key))
            .any(|key| key == issue_key)
}

fn cell_key_date(cell_key: &str) -> Option<NaiveDate> {
    cell_key
        .rsplit_once(':')
        .and_then(|(_, date)| NaiveDate::parse_from_str(date, "%Y-%m-%d").ok())
}

fn issue_key_from_cell_key(cell_key: &str) -> Option<&str> {
    cell_key.split_once(':').map(|(issue_key, _)| issue_key)
}

fn parse_timesheet_range_from_key(key: &str) -> Option<(NaiveDate, NaiveDate)> {
    let mut parts = key.split(':');
    let _account_id = parts.next()?;
    let kind = parts.next()?;
    if kind != "timesheet_data" {
        return None;
    }
    let start = NaiveDate::parse_from_str(parts.next()?, "%Y-%m-%d").ok()?;
    let end = NaiveDate::parse_from_str(parts.next()?, "%Y-%m-%d").ok()?;
    Some((start, end))
}

fn parse_week_monday_from_key(key: &str) -> Option<NaiveDate> {
    let mut parts = key.split(':');
    let _account_id = parts.next()?;
    let kind = parts.next()?;
    if kind != "week_cache" {
        return None;
    }
    NaiveDate::parse_from_str(parts.next()?, "%Y-%m-%d").ok()
}

fn snapshot_from_timesheet_data(ts: &TimesheetData, today: NaiveDate) -> RefreshSnapshot {
    let today_cell_keys = ts
        .bitbucket_activity
        .iter()
        .filter_map(|(cell_key, activity)| {
            cell_key_date(cell_key)
                .filter(|date| *date == today)
                .map(|_| (cell_key.clone(), activity.clone()))
        })
        .collect::<HashMap<_, _>>();
    let today_worklogs = ts
        .worklogs
        .iter()
        .filter(|entry| entry.date == today)
        .map(|entry| (entry.id.clone(), entry.clone()))
        .collect::<HashMap<_, _>>();
    let visible_keys = today_worklogs
        .values()
        .map(|entry| entry.issue_key.clone())
        .chain(
            today_cell_keys
                .keys()
                .filter_map(|cell_key| issue_key_from_cell_key(cell_key).map(ToString::to_string)),
        )
        .collect::<HashSet<_>>();
    let work_items = ts
        .work_items
        .iter()
        .filter(|item| visible_keys.contains(&item.key))
        .map(|item| (item.key.clone(), item.clone()))
        .collect::<HashMap<_, _>>();
    let ytd_hours = ts
        .ytd_hours
        .iter()
        .filter(|(key, _)| visible_keys.contains(*key))
        .map(|(key, value)| (key.clone(), *value))
        .collect::<HashMap<_, _>>();
    RefreshSnapshot {
        work_items,
        ytd_hours,
        worklogs: today_worklogs,
        bitbucket_activity: today_cell_keys,
    }
}

fn snapshot_from_current_week_cache(account_id: &str, today: NaiveDate) -> Option<RefreshSnapshot> {
    let key = cache::week_cache_key(account_id, week_monday(today));
    let raw = cache::get(&key)?;
    let ts = serde_json::from_str::<TimesheetData>(&raw).ok()?;
    Some(snapshot_from_timesheet_data(&ts, today))
}

fn apply_diff_to_timesheet_data(ts: &mut TimesheetData, diff: &TimesheetRefreshDiff) {
    let mut new_items = Vec::<WorkItem>::new();
    for item in &diff.work_items_upserted {
        if let Some(existing) = ts
            .work_items
            .iter_mut()
            .find(|existing| existing.key == item.key)
        {
            *existing = item.clone();
        } else {
            new_items.push(item.clone());
        }
    }
    for item in new_items.into_iter().rev() {
        ts.work_items.insert(0, item);
    }

    for update in &diff.ytd_hours_upserted {
        ts.ytd_hours.insert(update.issue_key.clone(), update.hours);
    }
    for key in &diff.ytd_hours_removed {
        ts.ytd_hours.remove(key);
    }

    if !diff.worklog_ids_removed.is_empty() {
        let removed = diff.worklog_ids_removed.iter().collect::<HashSet<_>>();
        ts.worklogs.retain(|entry| !removed.contains(&entry.id));
    }
    if !diff.worklogs_upserted.is_empty() {
        let upserted_ids = diff
            .worklogs_upserted
            .iter()
            .map(|entry| entry.id.as_str())
            .collect::<HashSet<_>>();
        ts.worklogs
            .retain(|entry| !upserted_ids.contains(entry.id.as_str()));
        ts.worklogs.extend(diff.worklogs_upserted.iter().cloned());
    }

    for cell_key in &diff.bitbucket_cell_keys_removed {
        ts.bitbucket_activity.remove(cell_key);
    }
    for update in &diff.bitbucket_activity_upserted {
        ts.bitbucket_activity
            .insert(update.cell_key.clone(), update.activity.clone());
    }

    for issue_key in &diff.work_item_keys_removed {
        if !has_timesheet_activity_for_key(ts, issue_key) {
            ts.work_items.retain(|item| item.key != *issue_key);
            ts.ytd_hours.remove(issue_key);
        }
    }

    crate::model::sort_work_items_for_timesheet(
        &mut ts.work_items,
        &ts.worklogs,
        &ts.bitbucket_activity,
    );
}

fn update_cached_timesheet_entries(
    account_id: &str,
    today: NaiveDate,
    diff: &TimesheetRefreshDiff,
) {
    cache::update_user_entries(account_id, |key, raw| {
        let should_patch = if let Some((start, end)) = parse_timesheet_range_from_key(key) {
            today >= start && today <= end
        } else if let Some(monday) = parse_week_monday_from_key(key) {
            today >= monday && today <= monday + chrono::Duration::days(6)
        } else {
            false
        };
        if !should_patch {
            return None;
        }
        let mut ts = serde_json::from_str::<TimesheetData>(raw).ok()?;
        apply_diff_to_timesheet_data(&mut ts, diff);
        serde_json::to_string(&ts).ok()
    });
}

fn build_refresh_diff(old: &RefreshSnapshot, new: &RefreshSnapshot) -> TimesheetRefreshDiff {
    let mut diff = TimesheetRefreshDiff::default();

    let mut work_item_keys = old
        .work_items
        .keys()
        .cloned()
        .chain(new.work_items.keys().cloned())
        .collect::<Vec<_>>();
    work_item_keys.sort();
    work_item_keys.dedup();
    for key in work_item_keys {
        match (old.work_items.get(&key), new.work_items.get(&key)) {
            (_, Some(new_item)) if old.work_items.get(&key) != Some(new_item) => {
                diff.work_items_upserted.push(new_item.clone());
            }
            (Some(_), None) => diff.work_item_keys_removed.push(key),
            _ => {}
        }
    }

    let mut ytd_keys = old
        .ytd_hours
        .keys()
        .cloned()
        .chain(new.ytd_hours.keys().cloned())
        .collect::<Vec<_>>();
    ytd_keys.sort();
    ytd_keys.dedup();
    for key in ytd_keys {
        match (old.ytd_hours.get(&key), new.ytd_hours.get(&key)) {
            (_, Some(new_value))
                if old
                    .ytd_hours
                    .get(&key)
                    .map(|value| (value - new_value).abs() < 1e-6)
                    != Some(true) =>
            {
                diff.ytd_hours_upserted.push(YtdHoursUpdate {
                    issue_key: key,
                    hours: *new_value,
                });
            }
            (Some(_), None) => diff.ytd_hours_removed.push(key),
            _ => {}
        }
    }

    let mut old_worklog_ids = old.worklogs.keys().cloned().collect::<Vec<_>>();
    old_worklog_ids.sort();
    for worklog_id in old_worklog_ids {
        if !new.worklogs.contains_key(&worklog_id) {
            diff.worklog_ids_removed.push(worklog_id);
        }
    }
    let mut new_worklog_ids = new.worklogs.keys().cloned().collect::<Vec<_>>();
    new_worklog_ids.sort();
    for worklog_id in new_worklog_ids {
        if let Some(new_entry) = new.worklogs.get(&worklog_id) {
            if old.worklogs.get(&worklog_id) != Some(new_entry) {
                diff.worklogs_upserted.push(new_entry.clone());
            }
        }
    }

    let mut old_cell_keys = old.bitbucket_activity.keys().cloned().collect::<Vec<_>>();
    old_cell_keys.sort();
    for cell_key in old_cell_keys {
        if !new.bitbucket_activity.contains_key(&cell_key) {
            diff.bitbucket_cell_keys_removed.push(cell_key);
        }
    }
    let mut new_cell_keys = new.bitbucket_activity.keys().cloned().collect::<Vec<_>>();
    new_cell_keys.sort();
    for cell_key in new_cell_keys {
        if let Some(new_activity) = new.bitbucket_activity.get(&cell_key) {
            if old.bitbucket_activity.get(&cell_key) != Some(new_activity) {
                diff.bitbucket_activity_upserted
                    .push(BitbucketActivityUpdate {
                        cell_key,
                        activity: new_activity.clone(),
                    });
            }
        }
    }

    diff
}

fn bitbucket_activity_cells_from_source(
    activity: &bitbucket::BitbucketActivity,
    show_merged_pr_activity: bool,
) -> HashMap<String, CellActivity> {
    let mut by_cell = HashMap::<String, CellActivity>::new();
    for (cell_key, commit_messages) in &activity.commit_messages_by_cell {
        let entry = by_cell.entry(cell_key.clone()).or_default();
        entry.commit_messages = commit_messages.clone();
    }
    for (cell_key, commit_links) in &activity.commit_links_by_cell {
        let entry = by_cell.entry(cell_key.clone()).or_default();
        entry.commit_links = commit_links.clone();
    }
    for (cell_key, test_result_links) in &activity.test_result_links_by_cell {
        let entry = by_cell.entry(cell_key.clone()).or_default();
        entry.test_result_links = test_result_links.clone();
    }
    for cell_key in &activity.pr_review_cells {
        if !show_merged_pr_activity && activity.pr_merged_cells.contains(cell_key) {
            continue;
        }
        let entry = by_cell.entry(cell_key.clone()).or_default();
        entry.has_pr_review = true;
    }
    for (cell_key, pr_links) in &activity.pr_links_by_cell {
        let entry = by_cell.entry(cell_key.clone()).or_default();
        entry.pr_links = pr_links.clone();
    }
    by_cell
}

fn update_cached_timesheet_entries_for_week(
    account_id: &str,
    monday: NaiveDate,
    diff: &TimesheetRefreshDiff,
) {
    let week_end = monday + chrono::Duration::days(6);
    cache::update_user_entries(account_id, |key, raw| {
        let should_patch = if let Some((start, end)) = parse_timesheet_range_from_key(key) {
            monday <= end && week_end >= start
        } else if let Some(cached_monday) = parse_week_monday_from_key(key) {
            cached_monday == monday
        } else {
            false
        };
        if !should_patch {
            return None;
        }
        let mut ts = serde_json::from_str::<TimesheetData>(raw).ok()?;
        apply_diff_to_timesheet_data(&mut ts, diff);
        serde_json::to_string(&ts).ok()
    });
}

async fn build_refresh_snapshot(
    creds: &jira::JiraCredentials,
    display_name: &str,
    today: NaiveDate,
    previous_keys: HashSet<String>,
) -> Result<RefreshSnapshot, String> {
    let bitbucket_handle = tokio::spawn({
        let email = creds.email.clone();
        let account_id = creds.account_id.clone();
        let display_name = display_name.to_string();
        async move {
            bitbucket::fetch_timesheet_activity_fresh_requested_window(
                &email,
                &account_id,
                &display_name,
                today,
                today,
            )
            .await
        }
    });
    let work_items_handle = tokio::spawn({
        let creds = creds.clone();
        async move { jira::fetch_work_items_fresh(&creds, today, today).await }
    });

    let bitbucket_activity = bitbucket_handle
        .await
        .map_err(|e| format!("Bitbucket refresh task failed: {}", e))??;
    let worklog_items = work_items_handle
        .await
        .map_err(|e| format!("Jira work item refresh task failed: {}", e))??;

    let worklog_item_map = worklog_items
        .into_iter()
        .map(|item| (item.key.clone(), item))
        .collect::<HashMap<_, _>>();
    let bitbucket_keys = bitbucket_activity
        .discovered_item_summaries
        .keys()
        .cloned()
        .collect::<HashSet<_>>();
    let candidate_keys = previous_keys
        .into_iter()
        .chain(worklog_item_map.keys().cloned())
        .chain(bitbucket_keys.iter().cloned())
        .collect::<HashSet<_>>();

    let worklog_tasks = candidate_keys
        .iter()
        .cloned()
        .map(|issue_key| {
            let creds = creds.clone();
            tokio::spawn(async move {
                let result = jira::fetch_worklogs_fresh(&creds, &issue_key, today, today).await;
                (issue_key, result)
            })
        })
        .collect::<Vec<_>>();

    let mut ytd_hours = HashMap::<String, f64>::new();
    let mut worklogs = HashMap::<String, WorklogEntry>::new();
    let mut worklog_visible_keys = HashSet::<String>::new();
    for task in worklog_tasks {
        let (issue_key, result) = task
            .await
            .map_err(|e| format!("Jira worklog refresh task failed: {}", e))?;
        let (entries, ytd_total) = result?;
        ytd_hours.insert(issue_key.clone(), ytd_total);
        if !entries.is_empty() {
            worklog_visible_keys.insert(issue_key);
            for entry in entries {
                worklogs.insert(entry.id.clone(), entry);
            }
        }
    }

    let mut bitbucket_activity_by_cell = HashMap::<String, CellActivity>::new();
    for (cell_key, commit_messages) in bitbucket_activity.commit_messages_by_cell {
        let entry = bitbucket_activity_by_cell.entry(cell_key).or_default();
        entry.commit_messages = commit_messages;
    }
    for (cell_key, commit_links) in bitbucket_activity.commit_links_by_cell {
        let entry = bitbucket_activity_by_cell.entry(cell_key).or_default();
        entry.commit_links = commit_links;
    }
    for (cell_key, test_result_links) in bitbucket_activity.test_result_links_by_cell {
        let entry = bitbucket_activity_by_cell.entry(cell_key).or_default();
        entry.test_result_links = test_result_links;
    }
    let prefs = crate::auth::load_user_prefs(&creds.account_id);
    let filtered_pr_review: HashSet<String> = if prefs.show_merged_pr_activity {
        bitbucket_activity.pr_review_cells
    } else {
        bitbucket_activity
            .pr_review_cells
            .difference(&bitbucket_activity.pr_merged_cells)
            .cloned()
            .collect()
    };
    for cell_key in filtered_pr_review {
        let entry = bitbucket_activity_by_cell.entry(cell_key).or_default();
        entry.has_pr_review = true;
    }
    for (cell_key, pr_links) in bitbucket_activity.pr_links_by_cell {
        let entry = bitbucket_activity_by_cell.entry(cell_key).or_default();
        entry.pr_links = pr_links;
    }

    let bitbucket_visible_keys = bitbucket_activity_by_cell
        .keys()
        .filter_map(|cell_key| issue_key_from_cell_key(cell_key).map(ToString::to_string))
        .collect::<HashSet<_>>();
    let visible_keys = worklog_visible_keys
        .union(&bitbucket_visible_keys)
        .cloned()
        .collect::<HashSet<_>>();

    let mut work_items = worklog_item_map;
    let missing_keys = visible_keys
        .iter()
        .filter(|key| !work_items.contains_key(*key))
        .cloned()
        .collect::<Vec<_>>();
    if !missing_keys.is_empty() {
        let fetched = jira::fetch_work_items_by_keys(creds, &missing_keys).await?;
        for item in fetched {
            work_items.insert(item.key.clone(), item);
        }
    }
    for key in visible_keys {
        if work_items.contains_key(&key) {
            continue;
        }
        let summary = bitbucket_activity
            .discovered_item_summaries
            .get(&key)
            .cloned()
            .unwrap_or_else(|| key.clone());
        work_items.insert(
            key.clone(),
            WorkItem {
                key,
                summary,
                icon_url: String::new(),
                issue_type: "Bitbucket".to_string(),
            },
        );
    }

    Ok(RefreshSnapshot {
        work_items,
        ytd_hours,
        worklogs,
        bitbucket_activity: bitbucket_activity_by_cell,
    })
}

async fn broadcast_diff(account_id: &str, diff: &TimesheetRefreshDiff, applied_at: &str) -> usize {
    let payload = match serde_json::to_string(&TimesheetWsMessage::RefreshDiff {
        diff: diff.clone(),
        applied_at: applied_at.to_string(),
    }) {
        Ok(payload) => payload,
        Err(err) => {
            log::warn!(
                "[periodic_refresh] failed to serialize diff payload for account={}: {}",
                account_id,
                err
            );
            return 0;
        }
    };
    let sessions = {
        let guard = ACTIVE_TIMESHEET_USERS.read().await;
        guard
            .get(account_id)
            .map(|user| user.sessions.values().cloned().collect::<Vec<_>>())
            .unwrap_or_default()
    };
    let mut sent = 0usize;
    for session in sessions {
        if session.tx.send(payload.clone()).is_ok() {
            sent += 1;
            log::info!(
                "[periodic_refresh] sent diff to session={} account={} changes={}",
                session.session_id,
                account_id,
                diff.change_count()
            );
        }
    }
    sent
}

async fn refresh_user(account_id: String, creds: jira::JiraCredentials, display_name: String) {
    let today = Local::now().date_naive();
    let old_snapshot = {
        let guard = ACTIVE_TIMESHEET_USERS.read().await;
        guard
            .get(&account_id)
            .and_then(|user| user.last_snapshot.clone())
            .unwrap_or_default()
    };
    let previous_keys = old_snapshot.visible_issue_keys();
    let new_snapshot =
        match build_refresh_snapshot(&creds, &display_name, today, previous_keys).await {
            Ok(snapshot) => snapshot,
            Err(err) => {
                log::warn!(
                    "[periodic_refresh] refresh failed account={} user={}: {}",
                    account_id,
                    display_name,
                    err
                );
                return;
            }
        };
    let old_snapshot_opt = {
        let guard = ACTIVE_TIMESHEET_USERS.read().await;
        guard
            .get(&account_id)
            .and_then(|user| user.last_snapshot.clone())
    };
    if let Some(old_snapshot) = old_snapshot_opt {
        let diff = build_refresh_diff(&old_snapshot, &new_snapshot);
        if !diff.is_empty() {
            update_cached_timesheet_entries(&account_id, today, &diff);
            let applied_at = chrono::Utc::now().to_rfc3339();
            let sent = broadcast_diff(&account_id, &diff, &applied_at).await;
            log::info!(
                "[periodic_refresh] account={} changes={} sessions_notified={}",
                account_id,
                diff.change_count(),
                sent
            );
        }
    }
    let mut guard = ACTIVE_TIMESHEET_USERS.write().await;
    if let Some(user) = guard.get_mut(&account_id) {
        user.last_snapshot = Some(new_snapshot);
    }
}

fn interval_label(active: bool, active_minutes: u64, idle_minutes: u64) -> String {
    if active {
        format!("active:{}m", active_minutes)
    } else {
        format!("idle:{}m", idle_minutes)
    }
}

// ─── Webhook integration ─────────────────────────────────────────────────────

/// Coalescing window for bursts of webhook events (pushes often arrive in
/// quick succession); one refresh cycle runs after the window closes.
const WEBHOOK_DEBOUNCE_SECS: u64 = 20;

static WEBHOOKS_ACTIVE: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
static WEBHOOK_REFRESH_SCHEDULED: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(false);

/// Marks webhook delivery as operational (registration succeeded). While
/// active, the polling loop stretches its active cadence to the fallback
/// interval since changes now arrive as pushed events.
pub fn set_webhooks_active(active: bool) {
    WEBHOOKS_ACTIVE.store(active, std::sync::atomic::Ordering::SeqCst);
}

pub fn webhooks_active() -> bool {
    WEBHOOKS_ACTIVE.load(std::sync::atomic::Ordering::SeqCst)
}

fn fallback_active_minutes() -> u64 {
    std::env::var("PERIODIC_REFRESH_FALLBACK_MINUTES")
        .ok()
        .and_then(|s| s.parse::<u64>().ok())
        .unwrap_or_else(idle_refresh_minutes)
        .max(1)
}

/// Entry point for the webhook receiver: debounce incoming events and run a
/// single refresh cycle for all active accounts once the window closes.
pub fn notify_webhook_event(event_key: String, repo_full_name: Option<String>) {
    log::debug!(
        "[periodic_refresh] webhook event {} repo={}",
        event_key,
        repo_full_name.as_deref().unwrap_or("?")
    );
    if WEBHOOK_REFRESH_SCHEDULED.swap(true, std::sync::atomic::Ordering::SeqCst) {
        return; // a refresh is already pending; this event is coalesced
    }
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_secs(WEBHOOK_DEBOUNCE_SECS)).await;
        WEBHOOK_REFRESH_SCHEDULED.store(false, std::sync::atomic::Ordering::SeqCst);
        bitbucket::invalidate_activity_cache();
        let refreshed = refresh_active_accounts().await;
        log::info!(
            "[periodic_refresh] webhook-triggered refresh done accounts={}",
            refreshed
        );
    });
}

/// Refresh every account that currently has an active WebSocket session.
/// Returns the number of accounts refreshed.
async fn refresh_active_accounts() -> usize {
    let active_accounts = {
        let guard = ACTIVE_TIMESHEET_USERS.read().await;
        guard.keys().cloned().collect::<HashSet<_>>()
    };
    if active_accounts.is_empty() {
        return 0;
    }
    let active_creds = auth::active_jira_credentials().await;
    let refresh_tasks = active_creds
        .into_iter()
        .filter(|user| active_accounts.contains(&user.creds.account_id))
        .map(|user| {
            let account_id = user.creds.account_id.clone();
            tokio::spawn(refresh_user(account_id, user.creds, user.display_name))
        })
        .collect::<Vec<_>>();
    let count = refresh_tasks.len();
    for task in refresh_tasks {
        if let Err(err) = task.await {
            log::warn!("[periodic_refresh] user refresh task join failed: {}", err);
        }
    }
    count
}

/// Force an immediate refresh for a specific account (called from the UI
/// "Force periodic refresh" button). No-op if the account has no active
/// WebSocket sessions.
pub async fn force_refresh_account(account_id: String) {
    let has_session = {
        let guard = ACTIVE_TIMESHEET_USERS.read().await;
        guard.contains_key(&account_id)
    };
    if !has_session {
        log::debug!(
            "[periodic_refresh] force_refresh_account: no active session for {}",
            account_id
        );
        return;
    }
    let active_creds = auth::active_jira_credentials().await;
    if let Some(user) = active_creds
        .into_iter()
        .find(|u| u.creds.account_id == account_id)
    {
        log::info!(
            "[periodic_refresh] force refresh for account={}",
            account_id
        );
        refresh_user(account_id, user.creds, user.display_name).await;
    }
}

async fn backfill_bitbucket_week_for_account(
    account_id: &str,
    creds: &jira::JiraCredentials,
    display_name: &str,
    monday: NaiveDate,
) {
    let week_end = monday + chrono::Duration::days(6);
    log::info!(
        "[periodic_refresh] bitbucket week backfill start account={} week={}",
        account_id,
        monday
    );
    let fetched = match bitbucket::fetch_timesheet_activity_fresh_requested_window(
        &creds.email,
        &creds.account_id,
        display_name,
        monday,
        week_end,
    )
    .await
    {
        Ok(activity) => activity,
        Err(err) => {
            log::warn!(
                "[periodic_refresh] bitbucket week backfill failed account={} week={} err={}",
                account_id,
                monday,
                err
            );
            return;
        }
    };

    let prefs = crate::auth::load_user_prefs(account_id);
    let cells = bitbucket_activity_cells_from_source(&fetched, prefs.show_merged_pr_activity);
    let mut diff = TimesheetRefreshDiff::default();
    diff.bitbucket_activity_upserted = cells
        .into_iter()
        .map(|(cell_key, activity)| BitbucketActivityUpdate { cell_key, activity })
        .collect::<Vec<_>>();

    let discovered = fetched
        .discovered_item_summaries
        .keys()
        .cloned()
        .collect::<Vec<_>>();
    if !discovered.is_empty() {
        let fetched_items = jira::fetch_work_items_by_keys(creds, &discovered)
            .await
            .unwrap_or_default();
        let mut by_key = fetched_items
            .into_iter()
            .map(|item| (item.key.clone(), item))
            .collect::<HashMap<_, _>>();
        for key in discovered {
            if by_key.contains_key(&key) {
                continue;
            }
            by_key.insert(
                key.clone(),
                WorkItem {
                    key: key.clone(),
                    summary: fetched
                        .discovered_item_summaries
                        .get(&key)
                        .cloned()
                        .unwrap_or_else(|| key.clone()),
                    icon_url: String::new(),
                    issue_type: "Bitbucket".to_string(),
                },
            );
        }
        diff.work_items_upserted = by_key.into_values().collect::<Vec<_>>();
    }

    update_cached_timesheet_entries_for_week(account_id, monday, &diff);
    cache::update_cached_bitbucket_week(account_id, monday, chrono::Utc::now());

    if !diff.is_empty() {
        let applied_at = chrono::Utc::now().to_rfc3339();
        let sent = broadcast_diff(account_id, &diff, &applied_at).await;
        log::info!(
            "[periodic_refresh] bitbucket week backfill done account={} week={} changes={} sessions_notified={}",
            account_id,
            monday,
            diff.change_count(),
            sent
        );
    } else {
        log::info!(
            "[periodic_refresh] bitbucket week backfill done account={} week={} no_changes=true",
            account_id,
            monday
        );
    }
}

pub fn queue_bitbucket_week_backfill(
    account_id: String,
    creds: jira::JiraCredentials,
    display_name: String,
    mondays: Vec<NaiveDate>,
) {
    let unique_mondays = mondays.into_iter().collect::<HashSet<_>>();
    for monday in unique_mondays {
        let inflight_key = format!("{}:{}", account_id, monday);
        let account_id_clone = account_id.clone();
        let creds_clone = creds.clone();
        let display_name_clone = display_name.clone();
        tokio::spawn(async move {
            {
                let mut inflight = BITBUCKET_WEEK_BACKFILL_INFLIGHT.lock().await;
                if inflight.contains(&inflight_key) {
                    return;
                }
                inflight.insert(inflight_key.clone());
            }
            backfill_bitbucket_week_for_account(
                &account_id_clone,
                &creds_clone,
                &display_name_clone,
                monday,
            )
            .await;
            let mut inflight = BITBUCKET_WEEK_BACKFILL_INFLIGHT.lock().await;
            inflight.remove(&inflight_key);
        });
    }
}

pub async fn run_periodic_refresh_loop() {
    let active_minutes = active_refresh_minutes();
    let idle_minutes = idle_refresh_minutes();
    let mut last_mode = None::<bool>;
    loop {
        let active_accounts = {
            let guard = ACTIVE_TIMESHEET_USERS.read().await;
            guard.keys().cloned().collect::<Vec<_>>()
        };
        let has_active_sessions = !active_accounts.is_empty();
        // With webhooks operational, polling is only a safety net: stretch
        // the active cadence to the fallback interval.
        let effective_active_minutes = if webhooks_active() {
            active_minutes.max(fallback_active_minutes())
        } else {
            active_minutes
        };
        if last_mode != Some(has_active_sessions) {
            log::info!(
                "[periodic_refresh] cadence now {}{}",
                interval_label(has_active_sessions, effective_active_minutes, idle_minutes),
                if webhooks_active() {
                    " (webhooks active, polling demoted to fallback)"
                } else {
                    ""
                }
            );
            last_mode = Some(has_active_sessions);
        }
        let interval = if has_active_sessions {
            Duration::from_secs(effective_active_minutes.saturating_mul(60))
        } else {
            Duration::from_secs(idle_minutes.saturating_mul(60))
        };

        // When idle, allow the first connecting session to interrupt the sleep
        // so a refresh fires immediately rather than after the full idle wait.
        let woken_by_session = if !has_active_sessions {
            tokio::select! {
                _ = tokio::time::sleep(interval) => false,
                _ = SESSION_BECAME_ACTIVE.notified() => true,
            }
        } else {
            tokio::time::sleep(interval).await;
            false
        };

        if woken_by_session {
            // A new session connected while we were in idle mode. Re-evaluate
            // cadence immediately, but do not run a refresh here: startup
            // already prewarms data and the periodic loop should wait for its
            // active interval before the first cycle.
            continue;
        }

        let active_accounts = {
            let guard = ACTIVE_TIMESHEET_USERS.read().await;
            guard.keys().cloned().collect::<Vec<_>>()
        };
        if active_accounts.is_empty() {
            continue;
        }
        log::info!(
            "[periodic_refresh] cycle start mode={} active_accounts={} reason={}",
            interval_label(
                !active_accounts.is_empty(),
                effective_active_minutes,
                idle_minutes
            ),
            active_accounts.len(),
            "timer"
        );
        refresh_active_accounts().await;
    }
}

pub async fn register_timesheet_session(
    snapshot: &AuthenticatedSessionSnapshot,
    tx: mpsc::UnboundedSender<String>,
) {
    let account_id = snapshot.user.account_id.clone();
    let session_id = snapshot.session_id.clone();
    let today = Local::now().date_naive();
    let before = {
        let guard = ACTIVE_TIMESHEET_USERS.read().await;
        guard
            .values()
            .map(|user| user.sessions.len())
            .sum::<usize>()
    };
    let mut guard = ACTIVE_TIMESHEET_USERS.write().await;
    let entry = guard.entry(account_id.clone()).or_default();
    entry.sessions.insert(
        session_id.clone(),
        TimesheetSessionSender {
            session_id: session_id.clone(),
            tx,
        },
    );
    if entry.last_snapshot.is_none() {
        entry.last_snapshot = snapshot_from_current_week_cache(&account_id, today);
    }
    let after = guard
        .values()
        .map(|user| user.sessions.len())
        .sum::<usize>();
    drop(guard);
    log::info!(
        "[periodic_refresh] timesheet websocket connected session={} account={} active_sessions={}",
        session_id,
        account_id,
        after
    );
    if before == 0 && after > 0 {
        log::info!(
            "[periodic_refresh] cadence change: active websocket sessions detected, switching to {}m interval",
            active_refresh_minutes()
        );
        SESSION_BECAME_ACTIVE.notify_one();
    }

    // Webhook housekeeping: retry registration if startup had no active
    // users, and run a catch-up refresh for events missed while this user
    // had no open session (polling is demoted while webhooks are active).
    let creds = snapshot.user.jira_credentials();
    let display_name = snapshot.user.display_name.clone();
    tokio::spawn(async move {
        crate::api::webhook::ensure_webhook_registration().await;
        if webhooks_active() {
            refresh_user(account_id, creds, display_name).await;
        }
    });
}

pub async fn unregister_timesheet_session(account_id: &str, session_id: &str) {
    let before = {
        let guard = ACTIVE_TIMESHEET_USERS.read().await;
        guard
            .values()
            .map(|user| user.sessions.len())
            .sum::<usize>()
    };
    let mut guard = ACTIVE_TIMESHEET_USERS.write().await;
    if let Some(user) = guard.get_mut(account_id) {
        user.sessions.remove(session_id);
        if user.sessions.is_empty() {
            guard.remove(account_id);
        }
    }
    let after = guard
        .values()
        .map(|user| user.sessions.len())
        .sum::<usize>();
    drop(guard);
    log::info!(
        "[periodic_refresh] timesheet websocket disconnected session={} account={} active_sessions={}",
        session_id,
        account_id,
        after
    );
    if before > 0 && after == 0 {
        log::info!(
            "[periodic_refresh] cadence change: no active websocket sessions remain, switching to {}m interval",
            idle_refresh_minutes()
        );
    }
}

pub async fn handle_timesheet_socket(socket: WebSocket, snapshot: AuthenticatedSessionSnapshot) {
    let account_id = snapshot.user.account_id.clone();
    let session_id = snapshot.session_id.clone();
    let (mut sender, mut receiver) = socket.split();
    let (tx, mut rx) = mpsc::unbounded_channel::<String>();
    register_timesheet_session(&snapshot, tx).await;
    let mut ping_interval = tokio::time::interval(Duration::from_secs(15));

    loop {
        tokio::select! {
            _ = ping_interval.tick() => {
                if sender.send(Message::Ping(Default::default())).await.is_err() {
                    break;
                }
            }
            outbound = rx.recv() => {
                match outbound {
                    Some(text) => {
                        if sender.send(Message::Text(text.into())).await.is_err() {
                            break;
                        }
                    }
                    None => break,
                }
            }
            inbound = receiver.next() => {
                match inbound {
                    Some(Ok(Message::Ping(bytes))) => {
                        if sender.send(Message::Pong(bytes)).await.is_err() {
                            break;
                        }
                    }
                    Some(Ok(Message::Pong(_))) => {}
                    Some(Ok(Message::Close(_))) => break,
                    Some(Ok(Message::Text(_))) | Some(Ok(Message::Binary(_))) => {}
                    Some(Err(err)) => {
                        log::warn!(
                            "[periodic_refresh] timesheet websocket error session={} account={}: {}",
                            session_id,
                            account_id,
                            err
                        );
                        break;
                    }
                    None => break,
                }
            }
        }
    }

    unregister_timesheet_session(&account_id, &session_id).await;
}
