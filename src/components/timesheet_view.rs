use crate::components::cell_popup::CellPopup;
use crate::components::popup_flush::{provide_popup_flush_context, use_popup_flush};
use crate::components::report_overlay::ReportOverlay;
use crate::components::settings_dialog::SettingsDialog;
use crate::components::timer::{
    PersistedTimerPopup, load_persisted_timer_popups, provide_timer_context,
};
use crate::components::week_navigator::{WeekNavigator, week_monday};
use crate::connection::use_connection;
use crate::formatting::{format_hours_long, format_hours_short};
use crate::i18n::{I18n, keys};
#[cfg(feature = "hydrate")]
use crate::model::TimesheetRefreshDiff;
#[cfg(feature = "hydrate")]
use crate::model::TimesheetWsMessage;
use crate::model::{ConnectionStatus, TimesheetData, WorkItem, WorklogEntry};
use chrono::{Datelike, Duration, Local, NaiveDate};
use leptos::prelude::*;

#[cfg(feature = "ssr")]
use crate::model::CellActivity;

// Import flag SVGs from shared flags module
use crate::flags::{FLAG_FR, FLAG_NL, FLAG_UK};

#[cfg(feature = "ssr")]
use std::collections::{HashMap, HashSet};

#[cfg(feature = "ssr")]
fn requested_week_mondays(start: NaiveDate, end: NaiveDate) -> Vec<NaiveDate> {
    let first = week_monday(start);
    let mut mondays = Vec::new();
    let mut cursor = first;
    while cursor <= end {
        mondays.push(cursor);
        cursor += Duration::weeks(1);
    }
    mondays
}

#[cfg(feature = "ssr")]
fn timesheet_for_week(source: &TimesheetData, monday: NaiveDate) -> TimesheetData {
    use std::collections::HashSet;
    let sunday = monday + Duration::days(6);
    let date_in_week = |d: NaiveDate| d >= monday && d <= sunday;

    let worklogs = source
        .worklogs
        .iter()
        .filter(|w| date_in_week(w.date))
        .cloned()
        .collect::<Vec<_>>();

    let bitbucket_activity = source
        .bitbucket_activity
        .iter()
        .filter_map(|(k, v)| {
            k.rsplit_once(':').and_then(|(_, date)| {
                NaiveDate::parse_from_str(date, "%Y-%m-%d")
                    .ok()
                    .filter(|d| date_in_week(*d))
                    .map(|_| (k.clone(), v.clone()))
            })
        })
        .collect::<HashMap<_, _>>();

    // Show all work_items (already filtered by Jira query for active status).
    // Jira returns items that match: worklogAuthor=user OR assignee=user with active status.
    // No need to filter by week activity here; active items remain visible all week.
    let work_items = source.work_items.clone();

    let visible_keys = work_items
        .iter()
        .map(|i| i.key.clone())
        .collect::<HashSet<_>>();

    let ytd_hours = source
        .ytd_hours
        .iter()
        .filter(|(k, _)| visible_keys.contains(k.as_str()))
        .map(|(k, v)| (k.clone(), *v))
        .collect::<HashMap<_, _>>();

    TimesheetData {
        work_items,
        worklogs,
        hours_per_week: source.hours_per_week,
        hours_per_day: source.hours_per_day,
        ytd_hours,
        bitbucket_activity,
        site_url: source.site_url.clone(),
        ..Default::default()
    }
}

#[cfg(feature = "ssr")]
fn merge_weekly_timesheets(chunks: Vec<TimesheetData>) -> TimesheetData {
    let mut by_key = HashMap::<String, WorkItem>::new();
    let mut worklogs = Vec::<WorklogEntry>::new();
    let mut ytd_hours = HashMap::<String, f64>::new();
    let mut bitbucket_activity = HashMap::<String, CellActivity>::new();
    let mut hours_per_week = 40.0;
    let mut hours_per_day = 8.0;
    let mut site_url = String::new();

    for chunk in chunks {
        hours_per_week = chunk.hours_per_week;
        hours_per_day = chunk.hours_per_day;
        if site_url.is_empty() {
            site_url = chunk.site_url.clone();
        }
        for item in chunk.work_items {
            by_key.insert(item.key.clone(), item);
        }
        for wl in chunk.worklogs {
            worklogs.push(wl);
        }
        for (k, v) in chunk.ytd_hours {
            ytd_hours.insert(k, v);
        }
        for (k, v) in chunk.bitbucket_activity {
            bitbucket_activity.entry(k).or_insert(v);
        }
    }

    let mut work_items = by_key.into_values().collect::<Vec<_>>();
    work_items.sort_by(|a, b| a.key.cmp(&b.key));

    TimesheetData {
        work_items,
        worklogs,
        hours_per_week,
        hours_per_day,
        ytd_hours,
        bitbucket_activity,
        site_url,
        ..Default::default()
    }
}

#[cfg(feature = "hydrate")]
fn timesheet_has_activity_for_issue(ts: &TimesheetData, issue_key: &str) -> bool {
    ts.worklogs.iter().any(|entry| entry.issue_key == issue_key)
        || ts
            .bitbucket_activity
            .keys()
            .filter_map(|cell_key| cell_key.split_once(':').map(|(key, _)| key))
            .any(|key| key == issue_key)
}

#[cfg(feature = "hydrate")]
fn apply_refresh_diff_to_timesheet(ts: &mut TimesheetData, diff: &TimesheetRefreshDiff) {
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
    for issue_key in &diff.ytd_hours_removed {
        ts.ytd_hours.remove(issue_key);
    }

    if !diff.worklog_ids_removed.is_empty() {
        let removed = diff
            .worklog_ids_removed
            .iter()
            .collect::<std::collections::HashSet<_>>();
        ts.worklogs.retain(|entry| !removed.contains(&entry.id));
    }
    if !diff.worklogs_upserted.is_empty() {
        let upsert_ids = diff
            .worklogs_upserted
            .iter()
            .map(|entry| entry.id.as_str())
            .collect::<std::collections::HashSet<_>>();
        ts.worklogs
            .retain(|entry| !upsert_ids.contains(entry.id.as_str()));
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
        if !timesheet_has_activity_for_issue(ts, issue_key) {
            ts.work_items.retain(|item| item.key != *issue_key);
            ts.ytd_hours.remove(issue_key);
        }
    }
}

#[cfg(feature = "hydrate")]
fn today_is_visible(selected_monday: NaiveDate, num_weeks: usize, today: NaiveDate) -> bool {
    let start = selected_monday - Duration::weeks((num_weeks.max(1) as i64) - 1);
    let end = selected_monday + Duration::days(6);
    today >= start && today <= end
}

#[cfg(feature = "hydrate")]
fn visible_range(selected_monday: NaiveDate, num_weeks: usize) -> (NaiveDate, NaiveDate) {
    let start = selected_monday - Duration::weeks((num_weeks.max(1) as i64) - 1);
    let end = selected_monday + Duration::days(6);
    (start, end)
}

#[cfg(feature = "hydrate")]
fn cell_key_date(cell_key: &str) -> Option<NaiveDate> {
    cell_key
        .rsplit_once(':')
        .and_then(|(_, date)| NaiveDate::parse_from_str(date, "%Y-%m-%d").ok())
}

#[cfg(feature = "hydrate")]
fn diff_affects_visible_range(
    diff: &TimesheetRefreshDiff,
    selected_monday: NaiveDate,
    num_weeks: usize,
    today: NaiveDate,
) -> bool {
    let (start, end) = visible_range(selected_monday, num_weeks);
    let mut has_dated_changes = false;

    for entry in &diff.worklogs_upserted {
        has_dated_changes = true;
        if entry.date >= start && entry.date <= end {
            return true;
        }
    }
    for update in &diff.bitbucket_activity_upserted {
        if let Some(date) = cell_key_date(&update.cell_key) {
            has_dated_changes = true;
            if date >= start && date <= end {
                return true;
            }
        }
    }
    for cell_key in &diff.bitbucket_cell_keys_removed {
        if let Some(date) = cell_key_date(cell_key) {
            has_dated_changes = true;
            if date >= start && date <= end {
                return true;
            }
        }
    }

    if has_dated_changes {
        return false;
    }

    today_is_visible(selected_monday, num_weeks, today)
}

#[cfg(feature = "hydrate")]
fn start_timesheet_refresh_socket(
    last_data: RwSignal<Option<TimesheetData>>,
    selected_monday: RwSignal<NaiveDate>,
    num_weeks: RwSignal<usize>,
    today: RwSignal<NaiveDate>,
    i18n: RwSignal<I18n>,
    refresh_toast: RwSignal<Option<String>>,
) {
    use wasm_bindgen::JsCast;
    use wasm_bindgen::closure::Closure;
    use web_sys::{CloseEvent, Event, MessageEvent, WebSocket};

    fn schedule_toast_clear(refresh_toast: RwSignal<Option<String>>) {
        let clear = Closure::wrap(Box::new(move || {
            refresh_toast.set(None);
        }) as Box<dyn FnMut()>);
        if let Some(window) = web_sys::window() {
            let _ = window.set_timeout_with_callback_and_timeout_and_arguments_0(
                clear.as_ref().unchecked_ref(),
                4000,
            );
            clear.forget();
        }
    }

    fn schedule_reconnect(
        last_data: RwSignal<Option<TimesheetData>>,
        selected_monday: RwSignal<NaiveDate>,
        num_weeks: RwSignal<usize>,
        today: RwSignal<NaiveDate>,
        i18n: RwSignal<I18n>,
        refresh_toast: RwSignal<Option<String>>,
    ) {
        let reconnect = Closure::wrap(Box::new(move || {
            start_timesheet_refresh_socket(
                last_data,
                selected_monday,
                num_weeks,
                today,
                i18n,
                refresh_toast,
            );
        }) as Box<dyn FnMut()>);
        if let Some(window) = web_sys::window() {
            let _ = window.set_timeout_with_callback_and_timeout_and_arguments_0(
                reconnect.as_ref().unchecked_ref(),
                5000,
            );
            reconnect.forget();
        }
    }

    let Some(window) = web_sys::window() else {
        return;
    };
    let location = window.location();
    let protocol = location.protocol().unwrap_or_else(|_| "http:".into());
    let ws_protocol = if protocol == "https:" { "wss:" } else { "ws:" };
    let host = location.host().unwrap_or_else(|_| "localhost:8081".into());
    let url = format!("{}//{}/ws/timesheet", ws_protocol, host);
    let Ok(ws) = WebSocket::new(&url) else {
        schedule_reconnect(
            last_data,
            selected_monday,
            num_weeks,
            today,
            i18n,
            refresh_toast,
        );
        return;
    };

    {
        let onmessage = Closure::<dyn Fn(MessageEvent)>::new(move |event: MessageEvent| {
            let Some(text) = event.data().as_string() else {
                return;
            };
            let Ok(message) = serde_json::from_str::<TimesheetWsMessage>(&text) else {
                return;
            };
            match message {
                TimesheetWsMessage::RefreshDiff { diff, .. } => {
                    if diff.is_empty() {
                        return;
                    }
                    if !diff_affects_visible_range(
                        &diff,
                        selected_monday.get_untracked(),
                        num_weeks.get_untracked(),
                        today.get_untracked(),
                    ) {
                        return;
                    }
                    let mut applied = false;
                    last_data.update(|opt| {
                        if let Some(ts) = opt.as_mut() {
                            apply_refresh_diff_to_timesheet(ts, &diff);
                            applied = true;
                        }
                    });
                    if applied {
                        refresh_toast.set(Some(i18n.get_untracked().t(keys::LIVE_REFRESH_APPLIED)));
                        schedule_toast_clear(refresh_toast);
                    }
                }
            }
        });
        ws.set_onmessage(Some(onmessage.as_ref().unchecked_ref()));
        onmessage.forget();
    }

    {
        let onclose = Closure::<dyn Fn(CloseEvent)>::new(move |_: CloseEvent| {
            schedule_reconnect(
                last_data,
                selected_monday,
                num_weeks,
                today,
                i18n,
                refresh_toast,
            );
        });
        ws.set_onclose(Some(onclose.as_ref().unchecked_ref()));
        onclose.forget();
    }

    {
        let onerror = Closure::<dyn Fn(Event)>::new(move |_: Event| {
            log::warn!("Timesheet WebSocket error");
        });
        ws.set_onerror(Some(onerror.as_ref().unchecked_ref()));
        onerror.forget();
    }

    {
        let onopen = Closure::<dyn Fn(Event)>::new(move |_: Event| {
            log::info!("Timesheet WebSocket opened");
        });
        ws.set_onopen(Some(onopen.as_ref().unchecked_ref()));
        onopen.forget();
    }
}

#[server(GetTimesheetData, "/api")]
pub async fn get_timesheet_data(
    start: NaiveDate,
    end: NaiveDate,
) -> Result<(TimesheetData, Option<(String, String)>), ServerFnError> {
    use crate::api::jira::timesheet_data_cache_key;
    use std::sync::Arc;
    let jira_started_at = std::time::Instant::now();

    let (_, session) = crate::auth::current_user_session().await?;
    let creds = Arc::new(session.jira_credentials());
    let user_profile = Some((session.avatar_url.clone(), session.display_name.clone()));
    let requested_mondays = requested_week_mondays(start, end);

    let cache_key = timesheet_data_cache_key(&creds.account_id, start, end);

    let mut cached_week_chunks = HashMap::<NaiveDate, TimesheetData>::new();
    // Week-cache fast path.
    let mut week_chunks = Vec::<TimesheetData>::new();
    for monday in &requested_mondays {
        let wkey = crate::api::cache::week_cache_key(&creds.account_id, *monday);
        let Some(raw) = crate::api::cache::get(&wkey) else {
            week_chunks.clear();
            break;
        };
        if let Ok(ts) = serde_json::from_str::<TimesheetData>(&raw) {
            cached_week_chunks.insert(*monday, ts.clone());
            week_chunks.push(ts);
        } else {
            week_chunks.clear();
            break;
        }
    }
    let mut bitbucket_cached_mondays = crate::api::cache::get_cached_bitbucket_weeks(&creds.account_id)
        .weeks
        .into_iter()
        .map(|week| week.monday)
        .collect::<HashSet<_>>();
    let recovered_bitbucket_cached_mondays = cached_week_chunks
        .iter()
        .filter(|(_, chunk)| !chunk.bitbucket_activity.is_empty())
        .map(|(monday, _)| *monday)
        .filter(|monday| !bitbucket_cached_mondays.contains(monday))
        .collect::<Vec<_>>();
    for monday in &recovered_bitbucket_cached_mondays {
        bitbucket_cached_mondays.insert(*monday);
        crate::api::cache::update_cached_bitbucket_week(
            &creds.account_id,
            *monday,
            chrono::Utc::now(),
        );
    }
    let missing_bitbucket_mondays = requested_mondays
        .iter()
        .cloned()
        .filter(|monday| !bitbucket_cached_mondays.contains(monday))
        .collect::<Vec<_>>();

    if week_chunks.len() == requested_mondays.len() && !week_chunks.is_empty() {
        let merged = merge_weekly_timesheets(week_chunks);
        if !missing_bitbucket_mondays.is_empty() {
            crate::api::periodic_refresh::queue_bitbucket_week_backfill(
                creds.account_id.clone(),
                creds.as_ref().clone(),
                session.display_name.clone(),
                missing_bitbucket_mondays.clone(),
            );
        }
        let selected_monday = end - chrono::Duration::days(6);
        let num_weeks = (((end - start).num_days() + 1) / 7).max(1) as usize;
        tokio::spawn(crate::api::jira::prefetch_adjacent_weeks(
            creds.clone(),
            session.display_name.clone(),
            selected_monday,
            num_weeks,
        ));
        return Ok((merged, user_profile));
    }

    // Check assembled-data cache first — this makes revisiting a week instant.
    if let Some(cached_json) = crate::api::cache::get(&cache_key) {
        if let Ok(ts) = serde_json::from_str::<TimesheetData>(&cached_json) {
            log::info!("[get_timesheet_data] cache hit for {} .. {}", start, end);
            if !missing_bitbucket_mondays.is_empty() {
                crate::api::periodic_refresh::queue_bitbucket_week_backfill(
                    creds.account_id.clone(),
                    creds.as_ref().clone(),
                    session.display_name.clone(),
                    missing_bitbucket_mondays.clone(),
                );
            }
            // Still prefetch neighbours so the next navigation is instant.
            let selected_monday = end - chrono::Duration::days(6);
            let num_weeks = (((end - start).num_days() + 1) / 7).max(1) as usize;
            tokio::spawn(crate::api::jira::prefetch_adjacent_weeks(
                creds.clone(),
                session.display_name.clone(),
                selected_monday,
                num_weeks,
            ));
            return Ok((ts, user_profile));
        }
    }

    // 1. Fetch Jira work items (per week) and merge.
    let jira_handles = requested_mondays
        .iter()
        .map(|monday| {
            let creds = creds.clone();
            let week_start = *monday;
            let week_end = *monday + Duration::days(6);
            tokio::spawn(async move {
                crate::api::jira::fetch_work_items(&creds, week_start, week_end).await
            })
        })
        .collect::<Vec<_>>();

    let mut jira_items_by_key = HashMap::<String, WorkItem>::new();
    for handle in jira_handles {
        let week_items = handle
            .await
            .map_err(|e| ServerFnError::new(format!("Jira fetch task failed: {}", e)))?
            .map_err(ServerFnError::new)?;
        for item in week_items {
            jira_items_by_key.entry(item.key.clone()).or_insert(item);
        }
    }
    let jira_items = jira_items_by_key.into_values().collect::<Vec<_>>();

    let mut all_items = jira_items;
    let mut bitbucket_activity: HashMap<String, CellActivity> = HashMap::new();
    // 1b. Use already-cached Bitbucket week slices for immediate response.
    for monday in &requested_mondays {
        let Some(chunk) = cached_week_chunks.get(monday) else {
            continue;
        };
        for (cell_key, activity) in &chunk.bitbucket_activity {
            bitbucket_activity.insert(cell_key.clone(), activity.clone());
        }
    }

    // 2. Fetch worklogs for all issues (per-issue cache handles dedup)
    let mut all_worklogs = Vec::new();
    let mut ytd_hours: HashMap<String, f64> = HashMap::new();
    for item in &all_items {
        match crate::api::jira::fetch_worklogs(&creds, &item.key, start, end).await {
            Ok((wls, total)) => {
                all_worklogs.extend(wls);
                ytd_hours.insert(item.key.clone(), total);
            }
            Err(e) => log::warn!("Failed to fetch worklogs for {}: {}", item.key, e),
        }
    }
    log::info!(
        "[get_timesheet_data] jira scan done: range={}..{}, work_items={}, bitbucket_cells_cached={}, worklog_issues={}, worklog_entries={}, elapsed_ms={}",
        start,
        end,
        all_items.len(),
        bitbucket_activity.len(),
        ytd_hours.len(),
        all_worklogs.len(),
        jira_started_at.elapsed().as_millis()
    );

    // Sort work items by key only (natural order).
    all_items.sort_by(|a, b| {
        fn parse_jira_key(key: &str) -> (&str, u64) {
            match key.rsplit_once('-') {
                Some((prefix, num)) => (prefix, num.parse().unwrap_or(0)),
                None => (key, 0),
            }
        }
        let (ap, an) = parse_jira_key(&a.key);
        let (bp, bn) = parse_jira_key(&b.key);
        ap.cmp(bp).then_with(|| an.cmp(&bn))
    });

    let ts = TimesheetData {
        work_items: all_items,
        worklogs: all_worklogs,
        hours_per_week: session.preferences.hours_per_week,
        hours_per_day: session.preferences.hours_per_day,
        ytd_hours,
        bitbucket_activity,
        site_url: session.site_url.clone(),
        ..Default::default()
    };

    // Cache the assembled result so the same week is instant next time.
    if let Ok(json) = serde_json::to_string(&ts) {
        crate::api::cache::put(cache_key.clone(), json);
    }
    for monday in requested_mondays {
        let week_key = crate::api::cache::week_cache_key(&creds.account_id, monday);
        let week_ts = timesheet_for_week(&ts, monday);
        if let Ok(raw) = serde_json::to_string(&week_ts) {
            crate::api::cache::put(week_key, raw);
            crate::api::cache::update_cached_week(&creds.account_id, monday, chrono::Utc::now());
            if bitbucket_cached_mondays.contains(&monday) {
                crate::api::cache::update_cached_bitbucket_week(
                    &creds.account_id,
                    monday,
                    chrono::Utc::now(),
                );
            }
        }
    }

    if !missing_bitbucket_mondays.is_empty() {
        crate::api::periodic_refresh::queue_bitbucket_week_backfill(
            creds.account_id.clone(),
            creds.as_ref().clone(),
            session.display_name.clone(),
            missing_bitbucket_mondays,
        );
    }

    // Prefetch adjacent weeks in the background so the next navigation
    // is instant.  This never blocks the current response.
    let selected_monday = end - chrono::Duration::days(6);
    let num_weeks = (((end - start).num_days() + 1) / 7).max(1) as usize;
    tokio::spawn(crate::api::jira::prefetch_adjacent_weeks(
        creds,
        session.display_name.clone(),
        selected_monday,
        num_weeks,
    ));

    Ok((ts, user_profile))
}

#[server(ClearCache, "/api")]
pub async fn clear_cache() -> Result<(), ServerFnError> {
    let (_, session) = crate::auth::current_user_session().await?;
    log::info!(
        "[clear_cache] clearing cache for user {}",
        session.account_id
    );
    crate::api::cache::remove_user_cache(&session.account_id);
    Ok(())
}

/// Trigger an immediate periodic refresh for the current user, bypassing the
/// normal timer. Returns immediately; any diffs are pushed via WebSocket.
#[server(ForcePeriodicRefresh, "/api")]
pub async fn force_periodic_refresh() -> Result<(), ServerFnError> {
    let (_, session) = crate::auth::current_user_session().await?;
    log::info!(
        "[force_periodic_refresh] triggered by user {}",
        session.account_id
    );
    #[cfg(feature = "ssr")]
    {
        let account_id = session.account_id.clone();
        tokio::spawn(async move {
            crate::api::periodic_refresh::force_refresh_account(account_id).await;
        });
    }
    Ok(())
}

#[server(GetIssueWorklogs, "/api")]
pub async fn get_issue_worklogs(
    issue_key: String,
    start: NaiveDate,
    end: NaiveDate,
) -> Result<(Vec<WorklogEntry>, f64), ServerFnError> {
    let (_, session) = crate::auth::current_user_session().await?;
    let creds = session.jira_credentials();
    // Invalidate the per-issue worklog cache and the assembled timesheet cache.
    crate::api::jira::invalidate_worklogs_for_issue(&creds.account_id, &issue_key);
    let (entries, ytd) = crate::api::jira::fetch_worklogs(&creds, &issue_key, start, end)
        .await
        .map_err(ServerFnError::new)?;
    Ok((entries, ytd))
}

#[server(SearchWorkItems, "/api")]
pub async fn search_work_items(query: String) -> Result<Vec<WorkItem>, ServerFnError> {
    let (_, session) = crate::auth::current_user_session().await?;
    let creds = session.jira_credentials();
    let items = crate::api::jira::search_issues(&creds, &query, 12)
        .await
        .map_err(|e| ServerFnError::new(e))?;
    Ok(items)
}

/// Day name key for the i18n system.
fn day_key(weekday_index: u32) -> &'static str {
    match weekday_index {
        0 => keys::MON,
        1 => keys::TUE,
        2 => keys::WED,
        3 => keys::THU,
        4 => keys::FRI,
        _ => keys::SAT,
    }
}

// ─── Popup state ────────────────────────────────────────────────────────────

/// Everything the popup needs, captured at click time so the popup can be
/// rendered independently of the table cells.
struct PopupInfo {
    /// Unique identifier so we can close / update individual popups.
    popup_id: u32,
    issue_key: String,
    issue_summary: String,
    date: NaiveDate,
    entries: Vec<WorklogEntry>,
    hours_per_day: f64,
    hours_per_week: f64,
    suggested_comments: Vec<String>,
    suggested_comment: Option<String>,
    commit_messages: Vec<String>,
    commit_links: Vec<String>,
    pr_links: Vec<String>,
    is_git_log: bool,
    is_weekend: bool,
    /// Whether the popup's date column is "today" (enables timer controls).
    is_today: bool,
    /// Inline CSS position computed at open time; updated by dragging.
    position_style: RwSignal<String>,
    /// The Jira site URL, used for worklog deep-link URLs.
    site_url: String,
    /// Optional timer draft restored from local storage.
    restored_timer_popup: Option<PersistedTimerPopup>,
}

impl Clone for PopupInfo {
    fn clone(&self) -> Self {
        Self {
            popup_id: self.popup_id,
            issue_key: self.issue_key.clone(),
            issue_summary: self.issue_summary.clone(),
            date: self.date,
            entries: self.entries.clone(),
            hours_per_day: self.hours_per_day,
            hours_per_week: self.hours_per_week,
            suggested_comments: self.suggested_comments.clone(),
            suggested_comment: self.suggested_comment.clone(),
            commit_messages: self.commit_messages.clone(),
            commit_links: self.commit_links.clone(),
            pr_links: self.pr_links.clone(),
            is_git_log: self.is_git_log,
            is_weekend: self.is_weekend,
            site_url: self.site_url.clone(),
            is_today: self.is_today,
            position_style: self.position_style.clone(),
            restored_timer_popup: self.restored_timer_popup.clone(),
        }
    }
}

/// Monotonically increasing counter for popup IDs.
static NEXT_POPUP_ID: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(1);

// ─── Viewport-aware popup positioning ───────────────────────────────────────

/// Compute an inline `left:…;top:…` style that places the popup near the
/// anchor cell identified by its `data-cell-key` / `data-cell-date` attributes
/// while keeping the popup fully inside the viewport.
///
/// On SSR the function is a no-op (returns empty string).
#[cfg(feature = "hydrate")]
fn compute_popup_style(issue_key: &str, date_str: &str, entry_count: usize) -> String {
    let Some(window) = web_sys::window() else {
        return String::new();
    };
    let Some(document) = window.document() else {
        return String::new();
    };

    let vw = window
        .inner_width()
        .ok()
        .and_then(|v| v.as_f64())
        .unwrap_or(1024.0);
    let vh = window
        .inner_height()
        .ok()
        .and_then(|v| v.as_f64())
        .unwrap_or(768.0);

    // Find the anchor cell span via data attributes.
    let selector = format!(
        r#"[data-cell-key="{}"][data-cell-date="{}"]"#,
        issue_key, date_str
    );
    let Some(el) = document.query_selector(&selector).ok().flatten() else {
        return String::new();
    };
    let rect = el.get_bounding_client_rect();

    let popup_width: f64 = 460.0; // 435px + padding and border
    // Rough height estimate: header + entries + new-entry row + buttons + padding.
    let popup_height_est: f64 = 150.0 + entry_count as f64 * 38.0;

    // Default: centred below the cell with a small gap.
    let mut left = rect.left() + rect.width() / 2.0 - popup_width / 2.0;
    let mut top = rect.top() + rect.height() + 4.0;

    // Keep within viewport horizontally.
    let margin = 8.0;
    if left + popup_width > vw - margin {
        left = vw - popup_width - margin;
    }
    if left < margin {
        left = margin;
    }

    // If it would overflow the bottom, flip to above the cell.
    if top + popup_height_est > vh - margin {
        top = rect.top() - popup_height_est - 4.0;
    }
    if top < margin {
        top = margin;
    }

    format!("left:{:.0}px;top:{:.0}px", left, top)
}

#[cfg(not(feature = "hydrate"))]
fn compute_popup_style(_issue_key: &str, _date_str: &str, _entry_count: usize) -> String {
    String::new()
}

/// Compute a `position:fixed` inline style for the search dropdown,
/// anchored just below the `.search-input` element.
#[cfg(feature = "hydrate")]
fn compute_search_dropdown_style() -> String {
    let Some(window) = web_sys::window() else {
        return String::new();
    };
    let Some(document) = window.document() else {
        return String::new();
    };
    let Some(el) = document.query_selector(".search-input").ok().flatten() else {
        return String::new();
    };
    let rect = el.get_bounding_client_rect();
    let left = rect.left();
    let top = rect.bottom();
    let width = rect.width().max(320.0);
    format!("left:{left:.0}px;top:{top:.0}px;width:{width:.0}px")
}

#[cfg(not(feature = "hydrate"))]
fn compute_search_dropdown_style() -> String {
    String::new()
}

#[cfg(feature = "hydrate")]
fn restored_popup_style(index: usize) -> String {
    let left = 32 + ((index % 3) as i32 * 36);
    let top = 96 + ((index % 6) as i32 * 28);
    format!("left:{left}px;top:{top}px")
}

#[cfg(not(feature = "hydrate"))]
fn restored_popup_style(_index: usize) -> String {
    String::new()
}

// ─── Viewport → number of week columns ─────────────────────────────────────

/// Compute how many week groups to display based on viewport width in pixels.
///
/// * ≤ 1000 px → 1 week
/// * 1001–1300 px → 2 weeks
/// * every additional 300 px → +1 week
#[cfg(feature = "hydrate")]
fn compute_num_weeks(viewport_width: f64) -> usize {
    let w = viewport_width as usize;
    if w > 1000 { 1 + (w - 701) / 300 } else { 1 }
}

// ─── Component ──────────────────────────────────────────────────────────────

#[component]
pub fn TimesheetView() -> impl IntoView {
    let i18n = use_context::<RwSignal<I18n>>().unwrap_or_else(|| {
        log::error!("I18n context not provided in TimesheetView, using English fallback");
        RwSignal::new(I18n::default())
    });

    // ── Timer context ──
    provide_timer_context();

    // ── Popup flush context ──
    provide_popup_flush_context();
    let flush_mgr = use_popup_flush();

    // Signals for user avatar and name
    let user_avatar = RwSignal::new(String::new());
    let user_name = RwSignal::new(String::new());
    let conn = use_connection();

    // ── Language selection logic ──
    use std::sync::Arc;
    let supported_langs = Arc::new(vec![
        ("en", "English", FLAG_UK),
        ("fr", "Français", FLAG_FR),
        ("nl", "Nederlands", FLAG_NL),
    ]);
    let lang_signal = RwSignal::new("".to_string());

    #[cfg(not(feature = "ssr"))]
    {
        if let Some(window) = web_sys::window() {
            let storage = window.local_storage().ok().flatten();
            let stored_lang = storage
                .as_ref()
                .and_then(|s| s.get_item("timesheet_lang").ok().flatten());
            let browser_lang = window
                .navigator()
                .language()
                .unwrap_or_else(|| "en".to_string());
            let mut lang = stored_lang.unwrap_or_else(|| browser_lang.clone());
            lang = lang.split('-').next().unwrap_or("en").to_lowercase();
            if !supported_langs.iter().any(|(code, _, _)| code == &lang) {
                lang = "en".to_string();
            }
            lang_signal.set(lang.clone());
            i18n.set(I18n::new(&lang));
        }
    }

    // Language change handler
    let on_lang_change = {
        let i18n = i18n.clone();
        let flush_mgr = flush_mgr.clone();
        move |new_lang: String| {
            #[cfg(not(feature = "ssr"))]
            {
                if let Some(window) = web_sys::window() {
                    if let Some(storage) = window.local_storage().ok().flatten() {
                        let _ = storage.set_item("timesheet_lang", &new_lang);
                    }
                }
            }

            // We need to reload the page after the language switch, but any
            // open-popup saves are async.  Use `flush_all_then` so the reload
            // only happens once every in-flight save has reached the server
            // (otherwise the reloaded page may serve stale cached data).
            let new_lang_inner = new_lang.clone();
            let i18n_inner = i18n.clone();
            let did_flush = flush_mgr.flush_all_then(move || {
                lang_signal.set(new_lang_inner.clone());
                i18n_inner.set(I18n::new(&new_lang_inner));
                #[cfg(not(feature = "ssr"))]
                {
                    web_sys::window().map(|w| w.location().reload().ok());
                }
            });

            // If flush_all_then returned false (re-entrancy) the callback
            // still fired synchronously, so the reload is already in
            // progress.  If no popups were dirty the latch fires
            // immediately as well.  Only when there *are* dirty popups do
            // we skip the immediate reload and let the latch handle it.
            if !did_flush {
                // Callback already ran inside flush_all_then.
            }
        }
    };

    // ── Language dropdown UI ──
    // Lang dropdown menu open state
    let lang_menu_open = RwSignal::new(false);

    let on_lang_btn_click = {
        let lang_menu_open = lang_menu_open.clone();
        move |_| {
            lang_menu_open.update(|open| *open = !*open);
        }
    };

    let on_lang_menu_blur = {
        let lang_menu_open = lang_menu_open.clone();
        move |_| {
            lang_menu_open.set(false);
        }
    };

    let lang_dropdown = || {
        let langs1 = supported_langs.clone();
        let langs3 = supported_langs.clone();
        view! {
            <div class="lang-dropdown">
                <button class="lang-btn" on:click=on_lang_btn_click tabindex="0">
                    <span inner_html={move || {
                        let current_lang = lang_signal.get();
                        langs1.iter().find(|(code, _, _)| *code == current_lang)
                            .map(|(_, _, flag)| *flag)
                            .unwrap_or(FLAG_UK)
                    }}></span>

                    <span class="lang-caret">
                        {move || if lang_menu_open.get() { "▲" } else { "▼" }}
                    </span>
                </button>
                <div
                    class=move || if lang_menu_open.get() { "lang-menu lang-menu-open" } else { "lang-menu" }
                    on:mouseleave=on_lang_menu_blur
                >
                    {move || {
                        let current_lang = lang_signal.get();
                        langs3.iter().map(|(code, name, flag)| {
                            let is_selected = *code == current_lang;
                            let on_click = {
                                let code = code.to_string();
                                let on_lang_change = on_lang_change.clone();
                                let lang_menu_open = lang_menu_open.clone();
                                move |_| {
                                    on_lang_change(code.clone());
                                    lang_menu_open.set(false);
                                }
                            };
                            view! {
                                <div class="lang-menu-item" class:lang-menu-item-selected=is_selected on:click=on_click>
                                    <span inner_html={*flag} title={*name}></span>
                                </div>
                            }
                        }).collect::<Vec<_>>()
                    }}
                </div>
            </div>
        }
    };

    let today = RwSignal::new(Local::now().date_naive());
    let selected_monday = RwSignal::new(week_monday(Local::now().date_naive()));
    // Keep selected_monday in sync with today
    Effect::new({
        let today = today.clone();
        let selected_monday = selected_monday.clone();
        move |_| {
            selected_monday.set(week_monday(today.get()));
        }
    });
    let is_refreshing = RwSignal::new(false);

    // ── Viewport-driven week count ──
    let num_weeks = RwSignal::new(1usize);

    #[cfg(feature = "hydrate")]
    {
        // Set initial value from current viewport width.
        if let Some(window) = web_sys::window() {
            if let Ok(w) = window.inner_width() {
                if let Some(w) = w.as_f64() {
                    num_weeks.set(compute_num_weeks(w));
                }
            }
        }

        // Update on resize.
        let resize_cb = wasm_bindgen::closure::Closure::<dyn Fn()>::new(move || {
            if let Some(window) = web_sys::window() {
                if let Ok(w) = window.inner_width() {
                    if let Some(w) = w.as_f64() {
                        let n = compute_num_weeks(w);
                        if num_weeks.get_untracked() != n {
                            num_weeks.set(n);
                        }
                    }
                }
            }
        });
        if let Some(window) = web_sys::window() {
            use wasm_bindgen::JsCast;
            let _ = window
                .add_event_listener_with_callback("resize", resize_cb.as_ref().unchecked_ref());
        }
        resize_cb.forget(); // intentional: lives for the app lifetime

        // --- Date change detection interval ---
        {
            use gloo_timers::callback::Interval;
            let today_signal = today.clone();
            Interval::new(60_000, move || {
                let now = Local::now().date_naive();
                if today_signal.get_untracked() != now {
                    today_signal.set(now);
                }
            })
            .forget();
        }
    }

    // The async Resource that fetches data when the selected week or the
    // number of visible weeks changes.
    let data = Resource::new(
        move || (selected_monday.get(), num_weeks.get()),
        |(monday, nw)| {
            let start = monday - Duration::weeks((nw as i64) - 1);
            let end = monday + Duration::days(6);
            get_timesheet_data(start, end)
        },
    );

    // ── Signals that keep the last successfully-loaded grid visible ──
    let last_data = RwSignal::new(Option::<TimesheetData>::None);
    let is_loading = RwSignal::new(true);
    let error_msg = RwSignal::new(Option::<String>::None);
    let refresh_toast = RwSignal::new(Option::<String>::None);

    #[cfg(feature = "hydrate")]
    start_timesheet_refresh_socket(
        last_data,
        selected_monday,
        num_weeks,
        today,
        i18n,
        refresh_toast,
    );

    // ── Work-item search state ──
    let search_query = RwSignal::new(String::new());
    let search_results = RwSignal::new(Vec::<WorkItem>::new());
    let show_search_dropdown = RwSignal::new(false);
    // Monotonically increasing version counter; a response is only applied
    // when its version still matches the latest value, effectively cancelling
    // all earlier in-flight requests.
    let search_version = RwSignal::new(0u32);

    // Clear search state whenever the user navigates to a different week.
    Effect::new(move |_| {
        let _ = selected_monday.get(); // track the signal
        search_query.set(String::new());
        search_results.set(vec![]);
        show_search_dropdown.set(false);
        search_version.set(search_version.get_untracked() + 1);
    });

    Effect::new(move |_| match data.get() {
        None => {
            is_loading.set(true);
        }
        Some(Ok((ts, user_profile))) => {
            last_data.set(Some(ts));
            if let Some((avatar, name)) = user_profile {
                log::info!("Setting user_avatar: {}, user_name: {}", avatar, name);
                user_avatar.set(avatar);
                user_name.set(name);
            }
            is_loading.set(false);
            error_msg.set(None);
        }
        Some(Err(e)) => {
            is_loading.set(false);
            error_msg.set(Some(e.to_string()));
        }
    });

    // ── Popup state — multiple popups can be open simultaneously ──
    let open_popups: RwSignal<Vec<PopupInfo>> = RwSignal::new(Vec::new());
    let restored_timer_popups_loaded = RwSignal::new(false);
    // Capture component-level owner so signals created during restore are not
    // owned by the Effect's reactive scope (which gets disposed on re-runs).
    let component_owner = Owner::current().expect("TimesheetView must run inside a reactive owner");
    let component_owner_for_restore = component_owner.clone();

    Effect::new(move |_| {
        let Some(ts) = last_data.get() else {
            return;
        };
        if restored_timer_popups_loaded.get() {
            return;
        }

        restored_timer_popups_loaded.set(true);

        let today_date = today.get_untracked();
        let mut restored = load_persisted_timer_popups();
        if restored.is_empty() {
            return;
        }

        open_popups.update(|popups| {
            for (index, draft) in restored.drain(..).enumerate() {
                if popups
                    .iter()
                    .any(|popup| popup.issue_key == draft.issue_key && popup.date == draft.date)
                {
                    continue;
                }

                let issue_summary = if draft.issue_summary.is_empty() {
                    ts.work_items
                        .iter()
                        .find(|item| item.key == draft.issue_key)
                        .map(|item| item.summary.clone())
                        .unwrap_or_default()
                } else {
                    draft.issue_summary.clone()
                };

                let entries = if draft.is_weekend {
                    ts.cell_worklogs(&draft.issue_key, draft.date)
                        .into_iter()
                        .chain(ts.cell_worklogs(&draft.issue_key, draft.date + Duration::days(1)))
                        .cloned()
                        .collect::<Vec<_>>()
                } else {
                    ts.cell_worklogs(&draft.issue_key, draft.date)
                        .into_iter()
                        .cloned()
                        .collect::<Vec<_>>()
                };

                let is_today = if draft.is_weekend {
                    today_date == draft.date || today_date == draft.date + Duration::days(1)
                } else {
                    today_date == draft.date
                };
                let mut row_pr_links = ts
                    .bitbucket_activity
                    .iter()
                    .filter_map(|(cell_key, activity)| {
                        cell_key
                            .strip_prefix(&format!("{}:", draft.issue_key))
                            .map(|_| activity.pr_links.clone())
                    })
                    .flatten()
                    .collect::<Vec<_>>();
                row_pr_links.sort();
                row_pr_links.dedup();

                popups.push(PopupInfo {
                    popup_id: NEXT_POPUP_ID.fetch_add(1, std::sync::atomic::Ordering::Relaxed),
                    issue_key: draft.issue_key.clone(),
                    issue_summary,
                    date: draft.date,
                    entries,
                    hours_per_day: ts.hours_per_day,
                    hours_per_week: ts.hours_per_week,
                    suggested_comments: draft
                        .suggested_comment
                        .clone()
                        .map(|s| vec![s])
                        .unwrap_or_default(),
                    suggested_comment: draft.suggested_comment.clone(),
                    commit_messages: Vec::new(),
                    commit_links: Vec::new(),
                    pr_links: row_pr_links,
                    is_git_log: draft.is_git_log,
                    is_weekend: draft.is_weekend,
                    is_today,
                    site_url: ts.site_url.clone(),
                    position_style: component_owner_for_restore.with(|| {
                        RwSignal::new(
                            draft
                                .position_style
                                .clone()
                                .filter(|style| !style.trim().is_empty())
                                .unwrap_or_else(|| restored_popup_style(index)),
                        )
                    }),
                    restored_timer_popup: Some(draft),
                });
            }
        });
    });

    // State for showing the settings dialog
    let show_settings = RwSignal::new(false);
    let show_report = RwSignal::new(false);

    // Detect date changes on window focus (for overnight transitions)
    #[cfg(feature = "hydrate")]
    {
        let today_signal = today.clone();
        let handle_focus = move |_: leptos::ev::FocusEvent| {
            let now = Local::now().date_naive();
            if today_signal.get() != now {
                today_signal.set(now);
                data.refetch();
            }
        };
        window_event_listener(leptos::ev::focus, handle_focus);
    }

    let on_popup_changed = Callback::new(move |issue_key: String| {
        // Targeted refresh: re-fetch only the changed issue's worklogs and
        // patch last_data in-place.  This avoids showing the loading overlay
        // and is much faster than a full refetch.
        #[cfg(feature = "hydrate")]
        {
            let monday = selected_monday.get_untracked();
            let nw = num_weeks.get_untracked();
            let start = monday - Duration::weeks((nw as i64) - 1);
            let end = monday + Duration::days(6);

            leptos::task::spawn_local(async move {
                match get_issue_worklogs(issue_key.clone(), start, end).await {
                    Ok((new_entries, new_ytd)) => {
                        last_data.update(|opt| {
                            if let Some(ts) = opt.as_mut() {
                                // Replace all worklogs for this issue.
                                ts.worklogs.retain(|w| w.issue_key != issue_key);
                                ts.worklogs.extend(new_entries);
                                ts.ytd_hours.insert(issue_key.clone(), new_ytd);
                            }
                        });
                    }
                    Err(e) => {
                        log::warn!("[on_popup_changed] get_issue_worklogs failed: {}", e);
                    }
                }
            });
        }
        #[cfg(not(feature = "hydrate"))]
        {
            let _ = issue_key;
        }
    });

    // ── Search input handler with debounce + cancellation ──
    let on_search_input = move |ev: leptos::ev::Event| {
        let value = event_target_value(&ev);
        search_query.set(value.clone());

        // Bump version to logically cancel any previous in-flight request.
        let v = search_version.get_untracked() + 1;
        search_version.set(v);

        let trimmed = value.trim().to_string();
        if trimmed.len() < 2 {
            search_results.set(vec![]);
            show_search_dropdown.set(false);
            return;
        }

        // Spawn an async task that waits 300 ms (debounce) then fires the
        // server call — but only if no newer keystroke has arrived.
        #[cfg(feature = "hydrate")]
        leptos::task::spawn_local(async move {
            // 300 ms debounce via a JS Promise-based sleep.
            let promise = js_sys::Promise::new(&mut |resolve, _| {
                if let Some(window) = web_sys::window() {
                    if window
                        .set_timeout_with_callback_and_timeout_and_arguments_0(&resolve, 300)
                        .is_err()
                    {
                        let _ = resolve.call0(&wasm_bindgen::JsValue::NULL);
                    }
                } else {
                    let _ = resolve.call0(&wasm_bindgen::JsValue::NULL);
                }
            });
            let _ = wasm_bindgen_futures::JsFuture::from(promise).await;

            // Abort if a newer keystroke arrived while we waited.
            if search_version.get_untracked() != v {
                return;
            }

            conn.request_started();
            let result = search_work_items(trimmed).await;
            conn.request_finished();

            match result {
                Ok(items) => {
                    // Only apply if still the latest request.
                    if search_version.get_untracked() == v {
                        let has_items = !items.is_empty();
                        search_results.set(items);
                        show_search_dropdown.set(has_items);
                    }
                }
                Err(_) => {
                    if search_version.get_untracked() == v {
                        search_results.set(vec![]);
                        show_search_dropdown.set(false);
                    }
                }
            }
        });
    };

    // ── Pick a work item from search results ──
    let on_pick_item = move |item: WorkItem| {
        // Close dropdown & clear search box.
        show_search_dropdown.set(false);
        search_query.set(String::new());
        search_results.set(vec![]);
        // Bump version so any still-pending request is discarded.
        search_version.set(search_version.get_untracked() + 1);

        // Add the item to the current timesheet data (if not already present).
        // Insert at the top of the list so it appears as the first row.
        last_data.update(|opt| {
            if let Some(ts) = opt.as_mut() {
                if !ts.work_items.iter().any(|w| w.key == item.key) {
                    ts.work_items.insert(0, item);
                }
            }
        });
    };

    // ── Refresh handler: clear server cache then refetch ──
    let on_refresh = {
        let flush_mgr = flush_mgr.clone();
        move |_| {
            is_refreshing.set(true);
            flush_mgr.flush_all_then(move || {
                #[cfg(feature = "hydrate")]
                leptos::task::spawn_local(async move {
                    conn.request_started();
                    let _ = clear_cache().await;
                    conn.request_finished();
                    data.refetch();
                    is_refreshing.set(false);
                });
            });
        }
    };

    let on_force_periodic_refresh = move |_| {
        #[cfg(feature = "hydrate")]
        leptos::task::spawn_local(async move {
            let _ = force_periodic_refresh().await;
        });
    };

    let on_open_settings = {
        let flush_mgr = flush_mgr.clone();
        move |_| {
            flush_mgr.flush_all();
            show_settings.set(true);
        }
    };
    let on_close_settings = Callback::new(move |_: ()| {
        show_settings.set(false);
    });
    let on_open_report = move |_| {
        show_report.set(true);
    };
    let on_close_report = Callback::new(move |_: ()| {
        show_report.set(false);
    });

    // ── beforeunload listener: flush open popups on page leave ──
    #[cfg(feature = "hydrate")]
    {
        let flush_mgr = flush_mgr.clone();
        let beforeunload_cb =
            wasm_bindgen::closure::Closure::<dyn Fn(web_sys::BeforeUnloadEvent)>::new(
                move |ev: web_sys::BeforeUnloadEvent| {
                    if flush_mgr.has_open_popups() {
                        flush_mgr.flush_all();
                        // Signal the browser that we'd like to show a
                        // confirmation dialog (best-effort; most browsers
                        // ignore the custom string but still show a prompt
                        // when preventDefault is called).
                        ev.prevent_default();
                    }
                },
            );
        if let Some(window) = web_sys::window() {
            use wasm_bindgen::JsCast;
            let _ = window.add_event_listener_with_callback(
                "beforeunload",
                beforeunload_cb.as_ref().unchecked_ref(),
            );
        }
        beforeunload_cb.forget();
    }

    view! {
        <div class="timesheet">

            // Timesheet title at the top, showing only user's first name
            {move || {
                let name = user_name.get();
                let first_name = name.split_whitespace().next().unwrap_or("").to_string();
                view! {
                    <h1>
                        {i18n.get().t(keys::TIMESHEET_TITLE)}
                        " "
                        {first_name}
                    </h1>
                }
            }}

            // Error banner (if any)
            {move || error_msg.get().map(|e| view! { <p class="error">{e}</p> })}

            // Main grid — rendered from last_data so the previous week stays
            // visible while the next week is loading.
            <div class="timesheet-grid-container">
                // Semi-transparent overlay while loading
                {move || is_loading.get().then(|| view! {
                    <div class="loading-overlay">
                        <span class="loading-spinner">{move || i18n.get().t(keys::LOADING_TIMESHEET)}</span>
                    </div>
                })}

                {move || {
                    let ts = match last_data.get() {
                        Some(ts) => ts,
                        None => return view! { <div /> }.into_any(),
                    };

                    let sel_monday = selected_monday.get();
                    let nw = num_weeks.get();

                    // Mondays for every visible week, oldest (leftmost) first.
                    let week_mondays: Vec<NaiveDate> = (0..nw)
                        .map(|i| sel_monday - Duration::weeks((nw - 1 - i) as i64))
                        .collect();

                    let i = i18n.get();
                    let dec_sep = i.decimal_separator;
                    let hpd = ts.hours_per_day;
                    let hpw = ts.hours_per_week;
                    let site_url = ts.site_url.clone();
                    let w_l = i.t(keys::WEEK_ABBR);
                    let d_l = i.t(keys::DAY_ABBR);
                    let h_l = i.t(keys::HOUR_ABBR);
                    let m_l = i.t(keys::MINUTE_ABBR);
                    let multi = nw > 1;

                    // ── Build header columns for every week group ──
                    let mut header_cols: Vec<AnyView> = Vec::new();
                    for (wi, wk_monday) in week_mondays.iter().enumerate() {
                        let wk_dates: Vec<NaiveDate> = (0..5)
                            .map(|d| *wk_monday + Duration::days(d))
                            .collect();

                        for (di, d) in wk_dates.iter().enumerate() {
                            let day_name = i.t(day_key(di as u32));
                            let date_str = i.format_day_month(d);
                            let total_str = format_hours_short(ts.day_total(*d), dec_sep);
                            let is_today = *d == today.get();
                            let cls = if is_today {
                                "col-day col-today"
                            } else if di == 0 && wi > 0 {
                                "col-day week-separator"
                            } else {
                                "col-day"
                            };
                            header_cols.push(view! {
                                <th class={cls}>
                                    <div class="day-name">{day_name}</div>
                                    <div class="day-date">{date_str}</div>
                                    <div class="day-total">{total_str}</div>
                                </th>
                            }.into_any());
                        }

                        let we_total_str = format_hours_short(ts.weekend_total(*wk_monday), dec_sep);
                        // Only highlight the weekend column as col-today if today is Sat or Sun AND this weekend column contains today
                        let today_date = today.get();
                        let is_today_weekend = (today_date == *wk_monday + Duration::days(5)) || (today_date == *wk_monday + Duration::days(6));
                        let weekend_cls = if is_today_weekend {
                            "col-weekend col-today"
                        } else {
                            "col-weekend"
                        };
                        header_cols.push(view! {
                            <th class={weekend_cls}>
                                <div class="day-name" title={i.t(keys::WEEKEND_TITLE)}>{i.t(keys::WEEKEND)}</div>
                                <div class="day-total">{we_total_str}</div>
                            </th>
                        }.into_any());

                        let wk_total_str = format_hours_short(ts.week_total(*wk_monday), dec_sep);
                        let total_label = if multi {
                            let wn = wk_monday.iso_week().week();
                            format!("{}{}", w_l.to_uppercase(), wn)
                        } else {
                            i.t(keys::TOTAL)
                        };
                        header_cols.push(view! {
                            <th class="col-total">
                                <div class="day-name">{total_label}</div>
                                <div class="day-total">{wk_total_str}</div>
                            </th>
                        }.into_any());
                    }

                    // ── Build body rows ──
                    let body_rows: Vec<AnyView> = ts.work_items
                        .iter()
                        .map(|item| {
                            let key = item.key.clone();
                            let item_ytd = ts.item_ytd_total(&key);

                            let header_total = {
                                let s = format_hours_short(item_ytd, dec_sep);
                                if s.is_empty() { String::new() } else { format!("{}", s) }
                            };

                            let icon_url = item.icon_url.clone();
                            let summary = item.summary.clone();
                            let _summary_for_title = summary.clone();
                            let key_display = key.clone();
                            let mut row_pr_links = ts
                                .bitbucket_activity
                                .iter()
                                .filter_map(|(cell_key, activity)| {
                                    cell_key
                                        .strip_prefix(&format!("{}:", key))
                                        .map(|_| activity.pr_links.clone())
                                })
                                .flatten()
                                .collect::<Vec<_>>();
                            row_pr_links.sort();
                            row_pr_links.dedup();

                            // Cells for every visible week group
                            let mut week_cells: Vec<AnyView> = Vec::new();
                            for (wi, wk_monday) in week_mondays.iter().enumerate() {
                                let wk_dates: Vec<NaiveDate> = (0..5)
                                    .map(|d| *wk_monday + Duration::days(d))
                                    .collect();

                                // ── Weekday cells ──
                                for (di, d) in wk_dates.iter().enumerate() {
                                    let hours = ts.cell_hours(&key, *d);
                                    let cell_text = format_hours_short(hours, dec_sep);
                                    let worklogs = ts.cell_worklogs(&key, *d);

                                    let bb_activity_for_closure = ts.bitbucket_activity.clone();
                                    let cell_activity = bb_activity_for_closure
                                        .get(&format!("{}:{}", key, d))
                                        .cloned()
                                        .unwrap_or_default();
                                    let commit_messages = cell_activity.commit_messages.clone();
                                    let has_pr_review = cell_activity.has_pr_review;

                                    let has_commit_associations = !cell_activity.commit_messages.is_empty()
                                        || !cell_activity.commit_links.is_empty();
                                    let show_corner_commit_overlay =
                                        !worklogs.is_empty() && has_commit_associations;
                                    let show_center_commit_overlay =
                                        worklogs.is_empty() && has_commit_associations;
                                    let show_center_pr_overlay =
                                        worklogs.is_empty() && !has_commit_associations && has_pr_review;
                                    let (cell_display, title) = if !worklogs.is_empty() {
                                        // Normal worklog cell
                                        let title = if worklogs.len() > 1 {
                                            worklogs
                                                .iter()
                                                .map(|w| {
                                                    let h = format_hours_long(w.hours, hpd, hpw, &w_l, &d_l, &h_l, &m_l);
                                                    // Strip work item key from start of comment if present
                                                    let comment = if let Some(stripped) = w.comment.strip_prefix(&format!("{} ", key)) {
                                                        stripped
                                                    } else {
                                                        w.comment.as_str()
                                                    };
                                                    if comment.is_empty() {
                                                        format!("{}", h)
                                                    } else {
                                                        format!("{}: {}", h, comment)
                                                    }
                                                })
                                                .collect::<Vec<_>>()
                                                .join("\n")
                                        } else {
                                            worklogs[0].comment.clone()
                                        };
                                        (cell_text.clone(), title)
                                    } else if !commit_messages.is_empty() {
                                        (String::new(), commit_messages.join("\n"))
                                    } else if has_pr_review {
                                        (String::new(), String::new())
                                    } else {
                                        (String::new(), String::new())
                                    };

                                    let cell_key = key.clone();
                                    let cell_date = *d;
                                    let cell_date_str = d.to_string();
                                    let cell_entries: Vec<_> = worklogs.into_iter().cloned().collect();
                                    let ck2 = cell_key.clone();
                                    let entries2 = cell_entries.clone();
                                    let is_today = cell_date == today.get();
                                    let cls = if is_today {
                                        "col-day timesheet-cell col-today"
                                    } else if di == 0 && wi > 0 {
                                        "col-day timesheet-cell week-separator"
                                    } else {
                                        "col-day timesheet-cell"
                                    };

                                    let cell_is_today = is_today;
                                    let cell_summary = summary.clone();
                                    let site_url_for_cell = site_url.clone();
                                    let owner_for_cell_popup = component_owner.clone();
                                    let row_pr_links_for_cell = row_pr_links.clone();
                                    week_cells.push(view! {
                                        <td class={cls} title={title}>
                                            <span
                                                class={if show_corner_commit_overlay {
                                                    "cell-value cell-value-with-commit-overlay-corner"
                                                } else if show_center_commit_overlay {
                                                    "cell-value cell-value-with-commit-overlay-center"
                                                } else {
                                                    "cell-value"
                                                }}
                                                data-cell-key={cell_key}
                                                data-cell-date={cell_date_str}
                                                on:click=move |_| {
                                                    // Don't open a duplicate popup for the same cell.
                                                    let already_open = open_popups.with(|ps| ps.iter().any(|p| p.issue_key == ck2 && p.date == cell_date));
                                                    if already_open {
                                                        return;
                                                    }
                                                    let (suggested_comments, is_git_log) = if entries2.is_empty() {
                                                        let activity = bb_activity_for_closure
                                                            .get(&format!("{}:{}", ck2, cell_date))
                                                            .cloned()
                                                            .unwrap_or_default();
                                                        if !activity.commit_messages.is_empty() {
                                                            (activity.commit_messages, true)
                                                        } else if activity.has_pr_review {
                                                            (vec!["review".to_string()], false)
                                                        } else {
                                                            (vec![], false)
                                                        }
                                                    } else {
                                                        (vec![], false)
                                                    };
                                                    let suggested_comment = if is_git_log {
                                                        suggested_comments.first().cloned()
                                                    } else {
                                                        None
                                                    };
                                                    let activity_links = bb_activity_for_closure
                                                        .get(&format!("{}:{}", ck2, cell_date))
                                                        .cloned()
                                                        .unwrap_or_default();
                                                    let pos_style = compute_popup_style(
                                                        &ck2,
                                                        &cell_date.to_string(),
                                                        entries2.len(),
                                                    );
                                                    let issue_summary = cell_summary.clone();
                                                    let popup = PopupInfo {
                                                        popup_id: NEXT_POPUP_ID.fetch_add(1, std::sync::atomic::Ordering::Relaxed),
                                                        issue_key: ck2.clone(),
                                                        issue_summary,
                                                        date: cell_date,
                                                        entries: entries2.clone(),
                                                        hours_per_day: hpd,
                                                        hours_per_week: hpw,
                                                        suggested_comments,
                                                        suggested_comment: suggested_comment.clone(),
                                                        commit_messages: activity_links.commit_messages.clone(),
                                                        commit_links: activity_links.commit_links,
                                                        pr_links: row_pr_links_for_cell.clone(),
                                                        is_git_log,
                                                        is_weekend: false,
                                                        is_today: cell_is_today,
                                                        position_style: owner_for_cell_popup
                                                            .with(|| RwSignal::new(pos_style)),
                                                        site_url: site_url_for_cell.clone(),
                                                        restored_timer_popup: None,
                                                    };
                                                    open_popups.update(|ps| ps.push(popup));
                                                }
                                            >
                                                {if show_corner_commit_overlay {
                                                    view! {
                                                        <>
                                                            <span class="cell-commit-overlay-mark cell-commit-overlay-mark--corner">{"c"}</span>
                                                            <span class="cell-value-text">{cell_display.clone()}</span>
                                                        </>
                                                    }
                                                        .into_any()
                                                } else if show_center_commit_overlay {
                                                    view! {
                                                        <>
                                                            <span class="cell-commit-overlay-mark cell-commit-overlay-mark--center">{"c"}</span>
                                                            <span class="cell-value-text"></span>
                                                        </>
                                                    }
                                                        .into_any()
                                                } else if show_center_pr_overlay {
                                                    view! {
                                                        <>
                                                            <span class="cell-commit-overlay-mark cell-pr-overlay-mark--center">{"p"}</span>
                                                            <span class="cell-value-text"></span>
                                                        </>
                                                    }
                                                        .into_any()
                                                } else {
                                                    view! { <>{cell_display}</> }.into_any()
                                                }}
                                            </span>
                                        </td>
                                    }.into_any());
                                }

                                // ── Weekend cell ──
                                let sat = *wk_monday + Duration::days(5);
                                let sun = *wk_monday + Duration::days(6);
                                let we_hours = ts.weekend_hours(&key, *wk_monday);
                                let we_text = format_hours_short(we_hours, dec_sep);
                                let we_worklogs: Vec<_> = ts.cell_worklogs(&key, sat)
                                    .into_iter()
                                    .chain(ts.cell_worklogs(&key, sun))
                                    .collect();

                                let we_title = if we_worklogs.len() > 1 {
                                    we_worklogs
                                        .iter()
                                        .map(|w| {
                                            let h = format_hours_long(w.hours, hpd, hpw, &w_l, &d_l, &h_l, &m_l);
                                            // Strip work item key from start of comment if present
                                            let comment = if let Some(stripped) = w.comment.strip_prefix(&format!("{} ", key)) {
                                                stripped
                                            } else {
                                                w.comment.as_str()
                                            };
                                            if comment.is_empty() {
                                                format!("{}", h)
                                            } else {
                                                format!("{}: {}", h, comment)
                                            }
                                        })
                                        .collect::<Vec<_>>()
                                        .join("\n")
                                } else if we_worklogs.len() == 1 {
                                    let h = format_hours_long(we_worklogs[0].hours, hpd, hpw, &w_l, &d_l, &h_l, &m_l);
                                    let comment = if let Some(stripped) = we_worklogs[0].comment.strip_prefix(&format!("{} ", key)) {
                                        stripped
                                    } else {
                                        we_worklogs[0].comment.as_str()
                                    };
                                    if comment.is_empty() {
                                        h
                                    } else {
                                        format!("{}: {}", h, comment)
                                    }
                                } else {
                                    String::new()
                                };


                                let we_entries: Vec<_> = we_worklogs.into_iter().cloned().collect();
                                let weekend_activity_sat = ts
                                    .bitbucket_activity
                                    .get(&format!("{}:{}", key, sat))
                                    .cloned()
                                    .unwrap_or_default();
                                let weekend_activity_sun = ts
                                    .bitbucket_activity
                                    .get(&format!("{}:{}", key, sun))
                                    .cloned()
                                    .unwrap_or_default();
                                let mut weekend_commit_messages = weekend_activity_sat.commit_messages;
                                weekend_commit_messages.extend(weekend_activity_sun.commit_messages);
                                let weekend_has_pr_review =
                                    weekend_activity_sat.has_pr_review || weekend_activity_sun.has_pr_review;
                                let weekend_has_commit_associations = !weekend_commit_messages.is_empty()
                                    || !weekend_activity_sat.commit_links.is_empty()
                                    || !weekend_activity_sun.commit_links.is_empty();
                                let show_corner_commit_overlay_weekend =
                                    !we_entries.is_empty() && weekend_has_commit_associations;
                                let show_center_commit_overlay_weekend =
                                    we_entries.is_empty() && weekend_has_commit_associations;
                                let show_center_pr_overlay_weekend = we_entries.is_empty()
                                    && !weekend_has_commit_associations
                                    && weekend_has_pr_review;
                                let (we_display, we_tooltip) = if !we_entries.is_empty() {
                                    (we_text.clone(), we_title.clone())
                                } else if !weekend_commit_messages.is_empty() {
                                    (String::new(), weekend_commit_messages.join("\n"))
                                } else if weekend_has_pr_review {
                                    (String::new(), String::new())
                                } else {
                                    (String::new(), String::new())
                                };

                                let we_key = key.clone();

                                let we_sat_str = sat.to_string();

                                let we_key2 = we_key.clone();

                                let we_entries2 = we_entries.clone();
                                let bb_activity_for_closure = ts.bitbucket_activity.clone();

                                // Only highlight the weekend cell as col-today if today is Sat or Sun AND this weekend cell contains today
                                let today_date = today.get();
                                let is_today_weekend = (today_date == sat) || (today_date == sun);
                                let weekend_cls = if is_today_weekend {
                                    "col-weekend timesheet-cell col-today"
                                } else {
                                    "col-weekend timesheet-cell"
                                };
                                let we_cell_is_today = is_today_weekend;
                                let we_summary = summary.clone();
                                let site_url_for_we = site_url.clone();
                                let owner_for_weekend_popup = component_owner.clone();
                                let row_pr_links_for_weekend = row_pr_links.clone();
                                week_cells.push(view! {
                                    <td class={weekend_cls} title={we_tooltip}>
                                        <span
                                            class={if show_corner_commit_overlay_weekend {
                                                "cell-value cell-value-with-commit-overlay-corner"
                                            } else if show_center_commit_overlay_weekend || show_center_pr_overlay_weekend {
                                                "cell-value cell-value-with-commit-overlay-center"
                                            } else {
                                                "cell-value"
                                            }}
                                            data-cell-key={we_key}
                                            data-cell-date={we_sat_str}

                                            on:click=move |_| {
                                                let already_open = open_popups.with(|ps| ps.iter().any(|p| p.issue_key == we_key2 && p.date == sat));
                                                if already_open {
                                                    return;
                                                }
                                                let (suggested_comments, is_git_log) = if we_entries2.is_empty() {
                                                    let sat_activity = bb_activity_for_closure
                                                        .get(&format!("{}:{}", we_key2, sat))
                                                        .cloned()
                                                        .unwrap_or_default();
                                                    let sun_activity = bb_activity_for_closure
                                                        .get(&format!("{}:{}", we_key2, sun))
                                                        .cloned()
                                                        .unwrap_or_default();
                                                    let mut commit_messages = sat_activity.commit_messages;
                                                    commit_messages.extend(sun_activity.commit_messages);
                                                    if !commit_messages.is_empty() {
                                                        (commit_messages, true)
                                                    } else if sat_activity.has_pr_review || sun_activity.has_pr_review {
                                                        (vec!["review".to_string()], false)
                                                    } else {
                                                        (vec![], false)
                                                    }
                                                } else {
                                                    (vec![], false)
                                                };
                                                let suggested_comment = if is_git_log {
                                                    suggested_comments.first().cloned()
                                                } else {
                                                    None
                                                };
                                                let sat_links = bb_activity_for_closure
                                                    .get(&format!("{}:{}", we_key2, sat))
                                                    .cloned()
                                                    .unwrap_or_default();
                                                let sun_links = bb_activity_for_closure
                                                    .get(&format!("{}:{}", we_key2, sun))
                                                    .cloned()
                                                    .unwrap_or_default();
                                                let mut weekend_commit_messages = sat_links.commit_messages.clone();
                                                weekend_commit_messages.extend(sun_links.commit_messages.clone());
                                                let mut weekend_commit_links = sat_links.commit_links;
                                                weekend_commit_links.extend(sun_links.commit_links);
                                                let mut seen_weekend_links = std::collections::HashSet::new();
                                                weekend_commit_links
                                                    .retain(|link| seen_weekend_links.insert(link.clone()));
                                                let pos_style = compute_popup_style(
                                                    &we_key2,
                                                    &sat.to_string(),
                                                    we_entries2.len(),
                                                );
                                                let issue_summary = we_summary.clone();
                                                let popup = PopupInfo {
                                                    popup_id: NEXT_POPUP_ID.fetch_add(1, std::sync::atomic::Ordering::Relaxed),
                                                    issue_key: we_key2.clone(),
                                                    issue_summary,
                                                    date: sat,
                                                    entries: we_entries2.clone(),
                                                    hours_per_day: hpd,
                                                    hours_per_week: hpw,
                                                    suggested_comments,
                                                    suggested_comment: suggested_comment.clone(),
                                                    commit_messages: weekend_commit_messages,
                                                    commit_links: weekend_commit_links,
                                                    pr_links: row_pr_links_for_weekend.clone(),
                                                    is_git_log,
                                                    is_weekend: true,
                                                    is_today: we_cell_is_today,
                                                    position_style: owner_for_weekend_popup
                                                        .with(|| RwSignal::new(pos_style)),
                                                    site_url: site_url_for_we.clone(),
                                                    restored_timer_popup: None,
                                                };
                                                open_popups.update(|ps| ps.push(popup));
                                            }

                                        >
                                            {if show_corner_commit_overlay_weekend {
                                                view! {
                                                    <>
                                                        <span class="cell-commit-overlay-mark cell-commit-overlay-mark--corner">{"c"}</span>
                                                        <span class="cell-value-text">{we_display.clone()}</span>
                                                    </>
                                                }
                                                    .into_any()
                                            } else if show_center_commit_overlay_weekend {
                                                view! {
                                                    <>
                                                        <span class="cell-commit-overlay-mark cell-commit-overlay-mark--center">{"c"}</span>
                                                        <span class="cell-value-text"></span>
                                                    </>
                                                }
                                                    .into_any()
                                            } else if show_center_pr_overlay_weekend {
                                                view! {
                                                    <>
                                                        <span class="cell-commit-overlay-mark cell-pr-overlay-mark--center">{"p"}</span>
                                                        <span class="cell-value-text"></span>
                                                    </>
                                                }
                                                    .into_any()
                                            } else {
                                                view! { <>{we_display}</> }.into_any()
                                            }}
                                        </span>
                                    </td>
                                }.into_any());

                                // ── Week total cell ──
                                let item_wk_total = ts.item_week_total(&key, *wk_monday);
                                week_cells.push(view! {
                                    <td class="col-total">
                                        {format_hours_short(item_wk_total, dec_sep)}
                                    </td>
                                }.into_any());
                            }

                            view! {
                                <tr>
                                    <td class="col-item"
                                        on:mouseenter=move |_ev: leptos::ev::MouseEvent| {
                                            #[cfg(feature = "hydrate")]
                                            {
                                                use wasm_bindgen::JsCast;
                                                if let Some(el) = _ev.target()
                                                    .and_then(|t| t.dyn_into::<web_sys::Element>().ok())
                                                {
                                                    if el.scroll_width() > el.client_width() {
                                                        let _ = el.set_attribute("title", &_summary_for_title);
                                                    } else {
                                                        let _ = el.remove_attribute("title");
                                                    }
                                                }
                                            }
                                        }
                                    >
                                        <img src={icon_url} class="issue-icon" width="12" height="12" alt={i18n.get().t(keys::ISSUE_ICON_ALT)} />
                                        <a
                                            href={format!("https://uplandsoftware.atlassian.net/browse/{}", key_display)}
                                            target="_blank"
                                            class="issue-key"
                                        >
                                            {key_display.clone()}
                                        </a>
                                        <span class="issue-summary">{summary}</span>
                                    </td>
                                    <td class="col-issue-total">
                                        {header_total}
                                    </td>
                                    {week_cells}
                                </tr>

                            }.into_any()
                        })
                        .collect();

                    // ── Build colgroup cols for table-layout:fixed ──
                    // col-item has no width so it absorbs whatever space remains after
                    // all fixed-width columns have claimed their share.
                    let mut colgroup_cols: Vec<AnyView> = Vec::new();
                    colgroup_cols.push(view! { <col class="col-item"></col> }.into_any());
                    colgroup_cols.push(view! { <col class="col-issue-total"></col> }.into_any());
                    for _ in 0..nw {
                        for _ in 0..5 {
                            colgroup_cols.push(view! { <col class="col-day"></col> }.into_any());
                        }
                        colgroup_cols.push(view! { <col class="col-weekend"></col> }.into_any());
                        colgroup_cols.push(view! { <col class="col-total"></col> }.into_any());
                    }

                    view! {
                        <div class="timesheet-table-wrap">
                            <table class="timesheet-grid">
                                <colgroup>{colgroup_cols}</colgroup>

                            <thead>
                                <tr>
                                    <th class="col-item" colspan="2">
                                        <input
                                            type="text"
                                            class="search-input"
                                            placeholder={move || i18n.get().t(keys::SEARCH_WORK_ITEM)}
                                            prop:value={move || search_query.get()}
                                            on:input=on_search_input.clone()
                                            on:blur=move |_| {
                                                show_search_dropdown.set(false);
                                            }
                                            on:keydown=move |ev: leptos::ev::KeyboardEvent| {
                                                if ev.key() == "Escape" {
                                                    show_search_dropdown.set(false);
                                                    search_query.set(String::new());
                                                    search_results.set(vec![]);
                                                    search_version.set(search_version.get_untracked() + 1);
                                                }
                                            }
                                        />
                                    </th>
                                    {header_cols}
                                </tr>

                                </thead>
                                <tbody>
                                    {body_rows}
                                </tbody>
                            </table>
                        </div>
                    }.into_any()
                }}
            </div>

            <div class="bottom-nav">
                <WeekNavigator selected_monday=selected_monday />
                <div class="nav-btn-group-right">
                    <button
                        class="nav-btn nav-report"
                        on:click=on_open_report
                        title=move || i18n.get().t(keys::USER_REPORT)
                    >
                        <span class="icon-report">{"📊"}</span>
                    </button>
                    <button
                        class="nav-btn nav-force-refresh"
                        on:click=on_force_periodic_refresh
                        title=move || i18n.get().t(keys::FORCE_PERIODIC_REFRESH)
                    >
                        <span class="icon-force-refresh">{"⚡"}</span>
                    </button>
                    <button
                        class="nav-btn nav-refresh"
                        on:click=on_refresh
                        disabled=move || is_refreshing.get()
                        title=move || i18n.get().t(keys::REFRESH_CACHED)
                    >
                        <span class="icon-refresh">{"🔄"}</span>
                    </button>
                    <button
                        class="nav-btn nav-settings"
                        on:click=on_open_settings
                        title=move || i18n.get().t(keys::OPEN_SETTINGS)
                    >
                        <span class="icon-settings">{"⚙️"}</span>
                    </button>
                    <a
                        class="nav-btn nav-logout"
                        href="/auth/logout"
                        title=move || i18n.get().t(keys::LOGOUT)
                    >
                        <span class="icon-logout">{"🚪"}</span>
                    </a>
                    {lang_dropdown()}
                    <span
                        class={move || match conn.status() {
                            ConnectionStatus::Online => "circle green",
                            ConnectionStatus::Waiting => "circle orange",
                            ConnectionStatus::Offline => "circle red",
                        }}
                        title={move || match conn.status() {
                            ConnectionStatus::Online => i18n.get().t(keys::CONNECTION_CONNECTED),
                            ConnectionStatus::Waiting => i18n.get().t(keys::CONNECTION_SYNCING),
                            ConnectionStatus::Offline => i18n.get().t(keys::CONNECTION_DISCONNECTED),
                        }}
                    ></span>
                </div>
            </div>


            {move || refresh_toast.get().map(|msg| view! {
                <div class="refresh-toast" role="status" aria-live="polite">{msg}</div>
            })}

            // ── Search dropdown rendered outside the table so it is never
            // clipped by overflow:hidden / sticky headers / stacking contexts.
            // Positioned with position:fixed relative to the .search-input element.
            {move || show_search_dropdown.get().then(|| {
                let items = search_results.get();
                let dd_style = compute_search_dropdown_style();
                view! {
                    <div class="search-dropdown-backdrop" on:mousedown=move |_| {
                        show_search_dropdown.set(false);
                    }></div>
                    <ul class="search-dropdown search-dropdown-positioned" style={dd_style}>
                        {items.into_iter().map(|item| {
                            let pick_item = item.clone();
                            let icon = item.icon_url.clone();
                            let key_label = item.key.clone();
                            let summary_label = item.summary.clone();
                            view! {
                                <li
                                    class="search-dropdown-item"
                                    on:mousedown=move |ev: leptos::ev::MouseEvent| {
                                        ev.prevent_default();
                                        on_pick_item(pick_item.clone());
                                    }
                                >
                                    <img src={icon.clone()} class="issue-icon" width="12" height="12" alt={i18n.get().t(keys::ISSUE_ICON_ALT)} />
                                    <span class="issue-key">{key_label.clone()}</span>
                                    <span class="issue-summary">{summary_label.clone()}</span>
                                </li>
                            }
                        }).collect::<Vec<_>>()}
                    </ul>
                }
            })}

            // ── Popups rendered outside the table ──
            // Multiple popups can be open simultaneously. Each is draggable
            // by its header bar and independently closeable.
            <For
                each={move || open_popups.get()}
                key={|info: &PopupInfo| info.popup_id}
                children={move |info: PopupInfo| {
                    let pid = info.popup_id;
                    let pos_sig = info.position_style;
                    let on_close_popup = Callback::new(move |_: ()| {
                        open_popups.update(|ps| ps.retain(|p| p.popup_id != pid));
                    });

                    view! {
                        <CellPopup
                           popup_id={pid}
                           pos_sig={pos_sig.clone()}
                           issue_key={info.issue_key}
                           issue_summary={info.issue_summary}
                           date={info.date}
                           entries={info.entries}
                           hours_per_day={info.hours_per_day}
                           hours_per_week={info.hours_per_week}
                           suggested_comments={info.suggested_comments.clone()}
                           suggested_comment={info.suggested_comment.clone()}
                           commit_messages={info.commit_messages.clone()}
                           commit_links={info.commit_links.clone()}
                           pr_links={info.pr_links.clone()}
                           is_git_log={info.is_git_log}
                           is_weekend={info.is_weekend}
                           restored_timer_popup={info.restored_timer_popup.clone()}
                           is_today={info.is_today}
                           on_close=on_close_popup
                           on_changed=on_popup_changed
                           site_url={info.site_url}
                        />
                    }
                }}
            />
            // Settings dialog modal
            {move || show_settings.get().then(|| view! {
                <SettingsDialog on_ok=on_close_settings on_cancel=on_close_settings />
            })}
            {move || show_report.get().then(|| view! {
                <ReportOverlay
                    hours_per_day={last_data.get().map(|d| d.hours_per_day).unwrap_or(8.0)}
                    hours_per_week={last_data.get().map(|d| d.hours_per_week).unwrap_or(40.0)}
                    on_close=on_close_report
                />
            })}
        </div>
    }
}
