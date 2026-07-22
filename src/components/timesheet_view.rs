use crate::components::cell_popup::CellPopup;
use crate::components::popup_flush::{provide_popup_flush_context, use_popup_flush};
use crate::components::report_overlay::{ReportRibbonControls, ReportView, create_report_state};
use crate::components::settings_dialog::SettingsDialog;
#[cfg(feature = "hydrate")]
use crate::components::settings_dialog::get_settings;
use crate::components::timer::{
    PersistedTimerPopup, load_persisted_timer_popups, provide_timer_context,
};
use crate::components::week_navigator::{WeekNavigator, week_monday};
use crate::connection::use_connection;
use crate::formatting::{format_hours_long, format_hours_short, parse_hours};
use crate::i18n::{I18n, keys};
#[cfg(feature = "hydrate")]
use crate::model::TimesheetRefreshDiff;
#[cfg(feature = "hydrate")]
use crate::model::TimesheetWsMessage;
use crate::model::{ConnectionStatus, CustomAction, TimesheetData, WorkItem, WorklogEntry};
use chrono::{Datelike, Duration, Local, NaiveDate};
use leptos::prelude::*;
#[cfg(feature = "hydrate")]
use leptos::web_sys;
use leptos_meta::Title;

use std::collections::{HashMap, HashSet};

#[cfg(feature = "ssr")]
use crate::model::CellActivity;

// Import flag SVGs from shared flags module
use crate::flags::{FLAG_FR, FLAG_NL, FLAG_UK};

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
    crate::model::sort_work_items_for_timesheet(&mut work_items, &worklogs, &bitbucket_activity);

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

    crate::model::sort_work_items_for_timesheet(
        &mut ts.work_items,
        &ts.worklogs,
        &ts.bitbucket_activity,
    );
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

fn pr_number(url: &str) -> Option<&str> {
    url.rsplit('/').next().filter(|s| !s.is_empty())
}

#[cfg_attr(not(feature = "hydrate"), allow(dead_code))]
fn cell_key_issue_key(cell_key: &str) -> Option<String> {
    cell_key
        .split_once(':')
        .map(|(issue_key, _)| issue_key.to_string())
}

#[derive(Clone, Debug, Default)]
struct RefreshToastPrUpdate {
    issue_key: String,
    pr_links: Vec<String>,
}

#[derive(Clone, Debug, Default)]
struct RefreshToastTestUpdate {
    issue_key: String,
    test_result_links: Vec<String>,
}

#[derive(Clone, Debug, Default)]
struct RefreshToastInfo {
    id: u64,
    hhmm: String,
    added_work_keys: Vec<String>,
    pr_updates: Vec<RefreshToastPrUpdate>,
    test_updates: Vec<RefreshToastTestUpdate>,
}

#[cfg_attr(not(feature = "hydrate"), allow(dead_code))]
static NEXT_REFRESH_TOAST_ID: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);

#[cfg_attr(not(feature = "hydrate"), allow(dead_code))]
fn hhmm_from_applied_at(applied_at: &str) -> String {
    chrono::DateTime::parse_from_rfc3339(applied_at)
        .map(|dt| dt.with_timezone(&chrono::Local).format("%H:%M").to_string())
        .unwrap_or_else(|_| chrono::Local::now().format("%H:%M").to_string())
}

#[cfg_attr(not(feature = "hydrate"), allow(dead_code))]
fn build_refresh_toast_info(
    diff: &crate::model::TimesheetRefreshDiff,
    existing_keys: &HashSet<String>,
    applied_at: &str,
) -> Option<RefreshToastInfo> {
    let mut added_work_keys = diff
        .work_items_upserted
        .iter()
        .map(|w| w.key.trim().to_uppercase())
        .filter(|key| !key.is_empty() && !existing_keys.contains(key))
        .collect::<Vec<_>>();
    added_work_keys.sort();
    added_work_keys.dedup();

    let mut pr_links_by_key = HashMap::<String, HashSet<String>>::new();
    let mut pr_keys_without_links = HashSet::<String>::new();
    let mut test_links_by_key = HashMap::<String, HashSet<String>>::new();

    for update in &diff.bitbucket_activity_upserted {
        let Some(issue_key) = cell_key_issue_key(&update.cell_key) else {
            continue;
        };
        let issue_key = issue_key.trim().to_uppercase();
        if issue_key.is_empty() {
            continue;
        }

        if update.activity.has_pr_review || !update.activity.pr_links.is_empty() {
            if update.activity.pr_links.is_empty() {
                pr_keys_without_links.insert(issue_key.clone());
            } else {
                let bucket = pr_links_by_key.entry(issue_key.clone()).or_default();
                for link in &update.activity.pr_links {
                    if !link.trim().is_empty() {
                        bucket.insert(link.clone());
                    }
                }
            }
        }

        if !update.activity.test_result_links.is_empty() {
            let bucket = test_links_by_key.entry(issue_key).or_default();
            for link in &update.activity.test_result_links {
                if !link.trim().is_empty() {
                    bucket.insert(link.clone());
                }
            }
        }
    }

    let mut pr_updates = pr_links_by_key
        .into_iter()
        .map(|(issue_key, links)| {
            let mut pr_links = links.into_iter().collect::<Vec<_>>();
            pr_links.sort();
            RefreshToastPrUpdate {
                issue_key,
                pr_links,
            }
        })
        .collect::<Vec<_>>();
    for issue_key in pr_keys_without_links {
        if !pr_updates.iter().any(|u| u.issue_key == issue_key) {
            pr_updates.push(RefreshToastPrUpdate {
                issue_key,
                pr_links: Vec::new(),
            });
        }
    }
    pr_updates.sort_by(|a, b| a.issue_key.cmp(&b.issue_key));

    let mut test_updates = test_links_by_key
        .into_iter()
        .map(|(issue_key, links)| {
            let mut test_result_links = links.into_iter().collect::<Vec<_>>();
            test_result_links.sort();
            RefreshToastTestUpdate {
                issue_key,
                test_result_links,
            }
        })
        .collect::<Vec<_>>();
    test_updates.sort_by(|a, b| a.issue_key.cmp(&b.issue_key));

    // Do not toast for commit-only or worklog-only refreshes.
    if added_work_keys.is_empty() && pr_updates.is_empty() && test_updates.is_empty() {
        return None;
    }

    Some(RefreshToastInfo {
        id: NEXT_REFRESH_TOAST_ID.fetch_add(1, std::sync::atomic::Ordering::Relaxed),
        hhmm: hhmm_from_applied_at(applied_at),
        added_work_keys,
        pr_updates,
        test_updates,
    })
}

fn attach_toast_stack_drag(ev: leptos::ev::MouseEvent, toast_stack_offset: RwSignal<(f64, f64)>) {
    #[cfg(feature = "hydrate")]
    {
        use wasm_bindgen::JsCast;
        use wasm_bindgen::closure::Closure;

        ev.prevent_default();
        let Some(window) = web_sys::window() else {
            return;
        };

        let start_x = ev.client_x() as f64;
        let start_y = ev.client_y() as f64;
        let (base_x, base_y) = toast_stack_offset.get_untracked();

        let move_cb = Closure::<dyn FnMut(web_sys::MouseEvent)>::wrap(Box::new({
            let toast_stack_offset = toast_stack_offset;
            move |mv: web_sys::MouseEvent| {
                let dx = (mv.client_x() as f64) - start_x;
                let dy = (mv.client_y() as f64) - start_y;
                toast_stack_offset.set((base_x + dx, base_y + dy));
            }
        }));

        let mouseup_cb = Closure::<dyn FnMut(web_sys::MouseEvent)>::wrap(Box::new({
            let window = window.clone();
            let move_ref = move_cb.as_ref().clone();
            move |_| {
                let _ = window
                    .remove_event_listener_with_callback("mousemove", move_ref.unchecked_ref());
            }
        }));

        let _ =
            window.add_event_listener_with_callback("mousemove", move_cb.as_ref().unchecked_ref());
        let _ =
            window.add_event_listener_with_callback("mouseup", mouseup_cb.as_ref().unchecked_ref());

        move_cb.forget();
        mouseup_cb.forget();
    }
    #[cfg(not(feature = "hydrate"))]
    {
        let _ = ev;
        let _ = toast_stack_offset;
    }
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
    refresh_toasts: RwSignal<Vec<RefreshToastInfo>>,
) {
    use wasm_bindgen::JsCast;
    use wasm_bindgen::closure::Closure;
    use web_sys::{CloseEvent, Event, MessageEvent, WebSocket};

    fn schedule_reconnect(
        last_data: RwSignal<Option<TimesheetData>>,
        selected_monday: RwSignal<NaiveDate>,
        num_weeks: RwSignal<usize>,
        today: RwSignal<NaiveDate>,
        refresh_toasts: RwSignal<Vec<RefreshToastInfo>>,
    ) {
        let reconnect = Closure::wrap(Box::new(move || {
            start_timesheet_refresh_socket(
                last_data,
                selected_monday,
                num_weeks,
                today,
                refresh_toasts,
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
        schedule_reconnect(last_data, selected_monday, num_weeks, today, refresh_toasts);
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
                TimesheetWsMessage::RefreshDiff { diff, applied_at } => {
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
                    let mut toast_info = None;
                    last_data.update(|opt| {
                        if let Some(ts) = opt.as_mut() {
                            let existing_keys = ts
                                .work_items
                                .iter()
                                .map(|w| w.key.trim().to_uppercase())
                                .collect::<HashSet<_>>();
                            toast_info =
                                build_refresh_toast_info(&diff, &existing_keys, &applied_at);
                            apply_refresh_diff_to_timesheet(ts, &diff);
                            applied = true;
                        }
                    });
                    if applied {
                        if let Some(toast) = toast_info {
                            refresh_toasts.update(|toasts| {
                                toasts.insert(0, toast);
                            });
                        }
                    }
                }
            }
        });
        ws.set_onmessage(Some(onmessage.as_ref().unchecked_ref()));
        onmessage.forget();
    }

    {
        let onclose = Closure::<dyn Fn(CloseEvent)>::new(move |_: CloseEvent| {
            schedule_reconnect(last_data, selected_monday, num_weeks, today, refresh_toasts);
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
    let mut bitbucket_cached_mondays =
        crate::api::cache::get_cached_bitbucket_weeks(&creds.account_id)
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
        if log::log_enabled!(log::Level::Debug) {
            let verify_creds = creds.clone();
            let verify_display_name = session.display_name.clone();
            let verify_ts = merged.clone();
            tokio::spawn(crate::api::jira::verify_cache_completeness(
                verify_creds,
                verify_display_name,
                verify_ts,
                start,
                end,
            ));
        }
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
            if log::log_enabled!(log::Level::Debug) {
                let verify_creds = creds.clone();
                let verify_display_name = session.display_name.clone();
                let verify_ts = ts.clone();
                tokio::spawn(crate::api::jira::verify_cache_completeness(
                    verify_creds,
                    verify_display_name,
                    verify_ts,
                    start,
                    end,
                ));
            }
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

    crate::model::sort_work_items_for_timesheet(&mut all_items, &all_worklogs, &bitbucket_activity);

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
    test_result_links: Vec<String>,
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
    /// Optional digit key that should seed first empty duration input.
    initial_digit: Option<char>,
}

#[cfg_attr(not(feature = "hydrate"), allow(dead_code))]
#[derive(Clone, Copy)]
enum PopupSeed {
    Hours(char),
    Description(char),
}

fn normalize_action_description(text: &str) -> String {
    text.split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase()
}

fn find_most_recent_matching_worklog(entries: &[WorklogEntry], description: &str) -> Option<usize> {
    let target = normalize_action_description(description);
    entries
        .iter()
        .enumerate()
        .filter(|(_, entry)| normalize_action_description(&entry.comment) == target)
        .map(|(idx, _)| idx)
        .last()
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
            test_result_links: self.test_result_links.clone(),
            pr_links: self.pr_links.clone(),
            is_git_log: self.is_git_log,
            is_weekend: self.is_weekend,
            site_url: self.site_url.clone(),
            is_today: self.is_today,
            position_style: self.position_style.clone(),
            restored_timer_popup: self.restored_timer_popup.clone(),
            initial_digit: self.initial_digit,
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

#[cfg(feature = "hydrate")]
fn today_col_index(
    selected_monday: NaiveDate,
    num_weeks: usize,
    today: NaiveDate,
) -> Option<usize> {
    let nw = num_weeks.max(1);
    let start_monday = selected_monday - Duration::weeks((nw as i64) - 1);
    for wi in 0..nw {
        let monday = start_monday + Duration::weeks(wi as i64);
        let friday = monday + Duration::days(4);
        let sunday = monday + Duration::days(6);
        if today >= monday && today <= friday {
            let weekday = (today - monday).num_days() as usize;
            return Some(wi * 6 + weekday);
        }
        if today > friday && today <= sunday {
            return Some(wi * 6 + 5);
        }
    }
    None
}

#[cfg(feature = "hydrate")]
fn focus_grid_cell(row: usize, col: usize) {
    use wasm_bindgen::JsCast;

    let Some(window) = web_sys::window() else {
        return;
    };
    let Some(document) = window.document() else {
        return;
    };
    let selector = format!(r#"[data-nav-row="{}"][data-nav-col="{}"]"#, row, col);
    let Some(node) = document.query_selector(&selector).ok().flatten() else {
        return;
    };
    let Some(el) = node.dyn_ref::<web_sys::HtmlElement>() else {
        return;
    };
    let _ = el.focus();
}

#[cfg(feature = "hydrate")]
fn schedule_digit_fill_for_popup(popup_id: u32, digit: char) {
    use wasm_bindgen::JsCast;
    use wasm_bindgen::closure::Closure;

    let Some(window) = web_sys::window() else {
        return;
    };
    let Some(document) = window.document() else {
        return;
    };
    let fill_cb = Closure::<dyn FnMut()>::new(move || {
        let popup_selector = format!(r#"[data-popup-id="{}"]"#, popup_id);
        let Some(popup_node) = document.query_selector(&popup_selector).ok().flatten() else {
            return;
        };
        let inputs = popup_node.get_elements_by_class_name("popup-hours");
        for idx in 0..inputs.length() {
            let Some(node) = inputs.item(idx) else {
                continue;
            };
            let Some(input) = node.dyn_ref::<web_sys::HtmlInputElement>() else {
                continue;
            };
            if input.value().trim().is_empty() {
                let value = digit.to_string();
                input.set_value(&value);
                if let Ok(ev) = web_sys::Event::new("input") {
                    let _ = input.dispatch_event(&ev);
                }
                let _ = input.focus();
                break;
            }
        }
    });
    let _ = window
        .set_timeout_with_callback_and_timeout_and_arguments_0(fill_cb.as_ref().unchecked_ref(), 0);
    fill_cb.forget();
}

#[cfg(feature = "hydrate")]
fn schedule_description_fill_for_popup(popup_id: u32, letter: char) {
    use wasm_bindgen::JsCast;
    use wasm_bindgen::closure::Closure;

    let Some(window) = web_sys::window() else {
        return;
    };
    let Some(document) = window.document() else {
        return;
    };
    let fill_cb = Closure::<dyn FnMut()>::new(move || {
        let popup_selector = format!(r#"[data-popup-id="{}"]"#, popup_id);
        let Some(popup_node) = document.query_selector(&popup_selector).ok().flatten() else {
            return;
        };
        for class_name in ["popup-comment", "popup-comment-new"] {
            let nodes = popup_node.get_elements_by_class_name(class_name);
            for idx in 0..nodes.length() {
                let Some(node) = nodes.item(idx) else {
                    continue;
                };
                let Some(input) = node.dyn_ref::<web_sys::HtmlTextAreaElement>() else {
                    continue;
                };
                if input.value().trim().is_empty() {
                    let value = letter.to_string();
                    input.set_value(&value);
                    if let Ok(ev) = web_sys::Event::new("input") {
                        let _ = input.dispatch_event(&ev);
                    }
                    let _ = input.focus();
                    return;
                }
            }
        }
    });
    let _ = window
        .set_timeout_with_callback_and_timeout_and_arguments_0(fill_cb.as_ref().unchecked_ref(), 0);
    fill_cb.forget();
}

#[cfg(feature = "hydrate")]
fn new_request_nonce() -> String {
    cfg_if::cfg_if! {
        if #[cfg(feature = "ssr")] {
            format!("{}:{}", chrono::Utc::now().timestamp(), uuid::Uuid::new_v4())
        } else {
            let ts = (js_sys::Date::now() / 1000.0) as i64;
            let r1 = (js_sys::Math::random() * f64::from(u32::MAX)) as u32;
            let r2 = (js_sys::Math::random() * f64::from(u32::MAX)) as u32;
            format!("{}:{:08x}{:08x}", ts, r1, r2)
        }
    }
}

fn custom_action_title(action: &CustomAction) -> String {
    let key = action.work_item_key.trim();
    let description = action.description.trim();
    let duration = action.duration.trim();
    let details = [description, duration]
        .into_iter()
        .filter(|value| !value.is_empty())
        .collect::<Vec<_>>()
        .join(" ");
    if !key.is_empty() && !details.is_empty() {
        format!("{}: {}", key, details)
    } else if !key.is_empty() {
        key.to_string()
    } else {
        details
    }
}

#[cfg(feature = "hydrate")]
fn schedule_custom_action_focus_existing_duration(popup_id: u32, existing_index: usize) {
    use wasm_bindgen::JsCast;
    use wasm_bindgen::closure::Closure;

    let Some(window) = web_sys::window() else {
        return;
    };
    let Some(document) = window.document() else {
        return;
    };
    let fill_cb = Closure::<dyn FnMut()>::new(move || {
        let popup_selector = format!(r#"[data-popup-id="{}"]"#, popup_id);
        let Some(popup_node) = document.query_selector(&popup_selector).ok().flatten() else {
            return;
        };
        let rows = popup_node.get_elements_by_class_name("popup-entry-group");
        let Some(row) = rows.item(existing_index as u32) else {
            return;
        };
        let Some(hours_node) = row
            .dyn_ref::<web_sys::Element>()
            .and_then(|el| el.query_selector(".popup-hours").ok().flatten())
        else {
            return;
        };
        let Some(input) = hours_node.dyn_ref::<web_sys::HtmlInputElement>() else {
            return;
        };
        let _ = input.focus();
    });
    let _ = window
        .set_timeout_with_callback_and_timeout_and_arguments_0(fill_cb.as_ref().unchecked_ref(), 0);
    fill_cb.forget();
}

#[cfg(feature = "hydrate")]
fn schedule_custom_action_toggle_existing_timer(popup_id: u32, existing_index: usize) {
    use wasm_bindgen::JsCast;
    use wasm_bindgen::closure::Closure;

    let Some(window) = web_sys::window() else {
        return;
    };
    let Some(document) = window.document() else {
        return;
    };
    let fill_cb = Closure::<dyn FnMut()>::new(move || {
        let popup_selector = format!(r#"[data-popup-id="{}"]"#, popup_id);
        let Some(popup_node) = document.query_selector(&popup_selector).ok().flatten() else {
            return;
        };
        let rows = popup_node.get_elements_by_class_name("popup-entry-group");
        let Some(row) = rows.item(existing_index as u32) else {
            return;
        };
        let Some(button_node) = row
            .dyn_ref::<web_sys::Element>()
            .and_then(|el| el.query_selector(".timer-play-pause").ok().flatten())
        else {
            return;
        };
        let Some(button) = button_node.dyn_ref::<web_sys::HtmlElement>() else {
            return;
        };
        let _ = button.click();
    });
    let _ = window
        .set_timeout_with_callback_and_timeout_and_arguments_0(fill_cb.as_ref().unchecked_ref(), 0);
    fill_cb.forget();
}

#[cfg(feature = "hydrate")]
fn schedule_custom_action_focus_new_duration(popup_id: u32, description: String) {
    use wasm_bindgen::JsCast;
    use wasm_bindgen::closure::Closure;

    let Some(window) = web_sys::window() else {
        return;
    };
    let Some(document) = window.document() else {
        return;
    };
    let fill_cb = Closure::<dyn FnMut()>::new(move || {
        let popup_selector = format!(r#"[data-popup-id="{}"]"#, popup_id);
        let Some(popup_node) = document.query_selector(&popup_selector).ok().flatten() else {
            return;
        };
        let rows = popup_node.get_elements_by_class_name("popup-new");
        for idx in 0..rows.length() {
            let Some(row) = rows.item(idx) else {
                continue;
            };
            let Some(comment_node) = row
                .dyn_ref::<web_sys::Element>()
                .and_then(|el| el.query_selector(".popup-comment-new").ok().flatten())
            else {
                continue;
            };
            let Some(comment_input) = comment_node.dyn_ref::<web_sys::HtmlTextAreaElement>() else {
                continue;
            };
            let existing_comment = comment_input.value();
            if existing_comment.trim().is_empty() || existing_comment.trim() == description {
                comment_input.set_value(&description);
                if let Ok(ev) = web_sys::Event::new("input") {
                    let _ = comment_input.dispatch_event(&ev);
                }
                if let Some(hours_input) = row
                    .dyn_ref::<web_sys::Element>()
                    .and_then(|el| el.query_selector(".popup-hours").ok().flatten())
                    .and_then(|node| node.dyn_ref::<web_sys::HtmlInputElement>().cloned())
                {
                    let _ = hours_input.focus();
                }
                break;
            }
        }
    });
    let _ = window
        .set_timeout_with_callback_and_timeout_and_arguments_0(fill_cb.as_ref().unchecked_ref(), 0);
    fill_cb.forget();
}

fn refresh_custom_actions(custom_actions: RwSignal<Vec<CustomAction>>) {
    #[cfg(feature = "hydrate")]
    leptos::task::spawn_local(async move {
        if let Ok(settings) = get_settings().await {
            custom_actions.set(
                settings
                    .custom_actions
                    .into_iter()
                    .filter(|action| !action.description.trim().is_empty())
                    .take(5)
                    .collect::<Vec<_>>(),
            );
        }
    });
    #[cfg(not(feature = "hydrate"))]
    let _ = custom_actions;
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
    let custom_actions = RwSignal::new(Vec::<CustomAction>::new());
    let conn = use_connection();
    refresh_custom_actions(custom_actions);

    // ── Language selection logic ──
    use std::sync::Arc;
    let supported_langs = Arc::new(vec![
        ("en", "English", FLAG_UK),
        ("fr", "Français", FLAG_FR),
        ("nl", "Nederlands", FLAG_NL),
    ]);
    let lang_signal = RwSignal::new("".to_string());

    #[cfg(feature = "hydrate")]
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
            #[cfg(feature = "hydrate")]
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
                #[cfg(feature = "hydrate")]
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

    let _lang_dropdown = || {
        let langs1 = supported_langs.clone();
        let langs3 = supported_langs.clone();
        view! {
            <div class="lang-dropdown">
                <button class="lang-btn" on:click=on_lang_btn_click tabindex="4">
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
    let refresh_toasts = RwSignal::new(Vec::<RefreshToastInfo>::new());
    let toast_stack_offset = RwSignal::new((0.0_f64, 0.0_f64));

    #[cfg(feature = "hydrate")]
    start_timesheet_refresh_socket(last_data, selected_monday, num_weeks, today, refresh_toasts);

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
                    test_result_links: Vec::new(),
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
                    initial_digit: None,
                });
            }
        });
    });

    // State for showing the settings dialog
    let show_settings = RwSignal::new(false);
    let show_report = RwSignal::new(false);
    let report_state = create_report_state();
    let user_menu_open = RwSignal::new(false);
    let focused_cell = RwSignal::new(Option::<(usize, usize)>::None);
    #[cfg(feature = "hydrate")]
    let initial_grid_focus_applied = RwSignal::new(false);

    #[cfg(feature = "hydrate")]
    Effect::new(move |_| {
        let nw = num_weeks.get().max(1);
        let today_date = today.get();
        let monday = selected_monday.get();
        let Some(ts) = last_data.get() else {
            focused_cell.set(None);
            return;
        };
        let row_count = ts.work_items.len();
        if row_count == 0 {
            focused_cell.set(None);
            return;
        }
        let col_count = nw * 6;
        let preferred_col =
            today_col_index(monday, nw, today_date).unwrap_or(col_count.saturating_sub(1));
        let middle_row = row_count / 2;
        focused_cell.update(|cur| match *cur {
            Some((r, c)) if r < row_count && c < col_count => {}
            _ => *cur = Some((middle_row, preferred_col)),
        });
    });

    #[cfg(feature = "hydrate")]
    Effect::new(move |_| {
        let Some((row, col)) = focused_cell.get() else {
            return;
        };
        if initial_grid_focus_applied.get() {
            return;
        }
        focus_grid_cell(row, col);
        initial_grid_focus_applied.set(true);
    });

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

    let on_force_periodic_refresh = move |_| {
        if !conn.is_available() {
            return;
        }
        #[cfg(feature = "hydrate")]
        leptos::task::spawn_local(async move {
            conn.request_started();
            let _ = force_periodic_refresh().await;
            conn.request_finished();
        });
    };

    let component_owner_for_action = component_owner.clone();
    let trigger_custom_action = Callback::new(move |index: usize| {
        if !conn.is_available() {
            return;
        }
        let Some(action) = custom_actions.get_untracked().get(index).cloned() else {
            return;
        };
        let Some(ts) = last_data.get_untracked() else {
            return;
        };
        if ts.work_items.is_empty() {
            return;
        }

        let focused_context = focused_cell.get_untracked().and_then(|(row, col)| {
            let nw = num_weeks.get_untracked().max(1);
            let col_count = nw * 6;
            if row >= ts.work_items.len() || col >= col_count {
                return None;
            }
            let week_idx = col / 6;
            let day_idx = col % 6;
            let start_monday =
                selected_monday.get_untracked() - Duration::weeks((nw.saturating_sub(1)) as i64);
            let monday = start_monday + Duration::weeks(week_idx as i64);
            let date = if day_idx == 5 {
                monday + Duration::days(5)
            } else {
                monday + Duration::days(day_idx as i64)
            };
            Some((ts.work_items[row].key.clone(), date, day_idx == 5))
        });

        let target_issue = {
            let explicit = action.work_item_key.trim().to_uppercase();
            if !explicit.is_empty() {
                explicit
            } else if let Some((issue_key, _, _)) = &focused_context {
                issue_key.clone()
            } else {
                ts.work_items[0].key.clone()
            }
        };
        let (target_date, is_weekend_cell) = if let Some((_, date, weekend)) = focused_context {
            (date, weekend)
        } else {
            (today.get_untracked(), false)
        };

        let target_entries = if is_weekend_cell {
            ts.cell_worklogs(&target_issue, target_date)
                .into_iter()
                .chain(ts.cell_worklogs(&target_issue, target_date + Duration::days(1)))
                .cloned()
                .collect::<Vec<_>>()
        } else {
            ts.cell_worklogs(&target_issue, target_date)
                .into_iter()
                .cloned()
                .collect::<Vec<_>>()
        };
        let trimmed_description = action.description.trim().to_string();
        let matching_index = find_most_recent_matching_worklog(&target_entries, &trimmed_description);
        let matching_entry = matching_index.and_then(|idx| target_entries.get(idx).cloned());
        let matching_has_duration = matching_entry
            .as_ref()
            .map(|entry| entry.hours > 0.0)
            .unwrap_or(false);

        let i = i18n.get_untracked();
        let action_duration_hours = if action.duration.trim().is_empty() {
            None
        } else {
            parse_hours(
                action.duration.trim(),
                ts.hours_per_day,
                ts.hours_per_week,
                i.decimal_separator,
                &i.t(keys::WEEK_ABBR),
                &i.t(keys::DAY_ABBR),
                &i.t(keys::HOUR_ABBR),
                &i.t(keys::MINUTE_ABBR),
            )
        };
        let action_has_duration = action_duration_hours.is_some();
        let target_is_today = if is_weekend_cell {
            let today_date = today.get_untracked();
            today_date == target_date || today_date == target_date + Duration::days(1)
        } else {
            today.get_untracked() == target_date
        };
        #[cfg(not(feature = "hydrate"))]
        let _ = (action_has_duration, target_is_today);

        if let Some(hours) = action_duration_hours
            && (!matching_has_duration
                || matching_entry.is_none())
        {
            let issue_key = target_issue.clone();
            let description = matching_entry
                .as_ref()
                .map(|entry| entry.comment.clone())
                .unwrap_or_else(|| trimmed_description.clone());
            let existing_id = matching_entry
                .as_ref()
                .map(|entry| entry.id.clone())
                .unwrap_or_default();
            let existing_adf = matching_entry.and_then(|entry| entry.comment_adf.clone());
            #[cfg(feature = "hydrate")]
            leptos::task::spawn_local(async move {
                conn.request_started();
                let result = if existing_id.is_empty() {
                    crate::components::cell_popup::server_add_worklog(
                        issue_key.clone(),
                        target_date,
                        hours,
                        description,
                        new_request_nonce(),
                    )
                    .await
                } else {
                    crate::components::cell_popup::server_update_worklog(
                        issue_key.clone(),
                        existing_id,
                        hours,
                        description,
                        existing_adf,
                        new_request_nonce(),
                    )
                    .await
                };
                conn.request_finished();
                if result.is_ok() {
                    on_popup_changed.run(issue_key);
                }
            });
            #[cfg(not(feature = "hydrate"))]
            {
                let _ = hours;
                let _ = issue_key;
                let _ = matching_has_duration;
                let _ = description;
                let _ = existing_id;
                let _ = existing_adf;
            }
            return;
        }

        let popup_id = if let Some(existing_id) = open_popups.with_untracked(|ps| {
            ps.iter()
                .find(|popup| popup.issue_key == target_issue && popup.date == target_date)
                .map(|popup| popup.popup_id)
        }) {
            existing_id
        } else {
            let issue_summary = ts
                .work_items
                .iter()
                .find(|item| item.key == target_issue)
                .map(|item| item.summary.clone())
                .unwrap_or_default();
            let mut pr_links = ts
                .bitbucket_activity
                .iter()
                .filter_map(|(cell_key, activity)| {
                    cell_key
                        .strip_prefix(&format!("{}:", target_issue))
                        .map(|_| activity.pr_links.clone())
                })
                .flatten()
                .collect::<Vec<_>>();
            pr_links.sort();
            pr_links.dedup();
            let popup_id = NEXT_POPUP_ID.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            let is_today = if is_weekend_cell {
                let today_date = today.get_untracked();
                today_date == target_date || today_date == target_date + Duration::days(1)
            } else {
                today.get_untracked() == target_date
            };
            let pos_style =
                compute_popup_style(&target_issue, &target_date.to_string(), target_entries.len());
            let popup = PopupInfo {
                popup_id,
                issue_key: target_issue.clone(),
                issue_summary,
                date: target_date,
                entries: target_entries.clone(),
                hours_per_day: ts.hours_per_day,
                hours_per_week: ts.hours_per_week,
                suggested_comments: Vec::new(),
                suggested_comment: None,
                commit_messages: Vec::new(),
                commit_links: Vec::new(),
                test_result_links: Vec::new(),
                pr_links,
                is_git_log: false,
                is_weekend: is_weekend_cell,
                is_today,
                position_style: component_owner_for_action.with(|| RwSignal::new(pos_style)),
                site_url: ts.site_url.clone(),
                restored_timer_popup: None,
                initial_digit: None,
            };
            open_popups.update(|ps| ps.push(popup));
            popup_id
        };

        #[cfg(feature = "hydrate")]
        match (action_has_duration, matching_index, target_is_today) {
            (true, Some(existing_index), true) => {
                schedule_custom_action_toggle_existing_timer(popup_id, existing_index);
            }
            (true, Some(existing_index), false) => {
                schedule_custom_action_focus_existing_duration(popup_id, existing_index);
            }
            (false, Some(existing_index), true) => {
                schedule_custom_action_toggle_existing_timer(popup_id, existing_index);
            }
            (false, Some(existing_index), false) => {
                schedule_custom_action_focus_existing_duration(popup_id, existing_index);
            }
            (false, None, _) => {
                schedule_custom_action_focus_new_duration(popup_id, trimmed_description.clone());
            }
            _ => {}
        }
        #[cfg(not(feature = "hydrate"))]
        let _ = popup_id;
    });

    let on_settings_saved = Callback::new(move |_: ()| {
        refresh_custom_actions(custom_actions);
        show_settings.set(false);
    });
    let on_close_settings = Callback::new(move |_: ()| {
        show_settings.set(false);
    });
    let on_open_report = move |_| {
        show_report.set(true);
    };

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

    let flush_mgr_for_left = flush_mgr.clone();
    let flush_mgr_for_right = flush_mgr.clone();
    let report_state_for_ribbon = report_state.clone();
    let report_state_for_view = report_state.clone();

    // ── Global hotkey: Alt+L focuses last selected worklog cell ──
    #[cfg(feature = "hydrate")]
    {
        use wasm_bindgen::JsCast;

        let focused_cell_for_hotkey = focused_cell;
        let selected_monday_for_hotkey = selected_monday;
        let today_for_hotkey = today;
        let show_report_for_hotkey = show_report;
        let report_state_for_hotkey = report_state.clone();
        let flush_mgr_for_hotkey = flush_mgr.clone();
        let conn_for_hotkey = conn.clone();
        let trigger_custom_action_for_hotkey = trigger_custom_action;
        let focus_last_cell_cb =
            wasm_bindgen::closure::Closure::<dyn Fn(web_sys::KeyboardEvent)>::new(
                move |ev: web_sys::KeyboardEvent| {
                    let key = ev.key();
                    if !ev.alt_key() || ev.ctrl_key() || ev.meta_key() {
                        return;
                    }

                    if show_report_for_hotkey.get_untracked() {
                        match key.to_ascii_lowercase().as_str() {
                            "p" => {
                                ev.prevent_default();
                                if report_state_for_hotkey.period.get_untracked()
                                    == crate::components::report_overlay::ReportPeriod::Week
                                {
                                    report_state_for_hotkey.selected_month.update(|m| {
                                        *m = crate::components::report_overlay::previous_month(*m)
                                    });
                                } else {
                                    report_state_for_hotkey.selected_year.update(|y| *y -= 1);
                                }
                            }
                            "n" => {
                                ev.prevent_default();
                                if report_state_for_hotkey.period.get_untracked()
                                    == crate::components::report_overlay::ReportPeriod::Week
                                {
                                    report_state_for_hotkey.selected_month.update(|m| {
                                        *m = crate::components::report_overlay::next_month(*m)
                                    });
                                } else {
                                    report_state_for_hotkey.selected_year.update(|y| *y += 1);
                                }
                            }
                            "d" => {
                                ev.prevent_default();
                                if let Some(window) = web_sys::window() {
                                    if let Some(document) = window.document() {
                                        if let Some(node) = document
                                            .query_selector(".report-controls select")
                                            .ok()
                                            .flatten()
                                        {
                                            if let Some(el) = node.dyn_ref::<web_sys::HtmlElement>()
                                            {
                                                let _ = el.click();
                                            }
                                        }
                                    }
                                }
                            }
                            "t" => {
                                ev.prevent_default();
                                let today_date = Local::now().date_naive();
                                report_state_for_hotkey.selected_month.set(
                                    crate::components::report_overlay::default_report_month(
                                        today_date,
                                    ),
                                );
                                report_state_for_hotkey.selected_year.set(today_date.year());
                            }
                            "s" => {
                                ev.prevent_default();
                                flush_mgr_for_hotkey.flush_all();
                                show_settings.set(true);
                            }
                            "w" => {
                                ev.prevent_default();
                                show_report.set(false);
                            }
                            "x" => {
                                ev.prevent_default();
                                if let Some(window) = web_sys::window() {
                                    let _ = window.location().set_href("/auth/logout");
                                }
                            }
                            _ => {}
                        }
                    } else {
                        let key_lower = key.to_ascii_lowercase();
                        if key_lower.len() == 1 {
                            if let Some(digit) =
                                key_lower.chars().next().filter(|c| ('1'..='5').contains(c))
                            {
                                ev.prevent_default();
                                let idx = (digit as u8 - b'1') as usize;
                                trigger_custom_action_for_hotkey.run(idx);
                                return;
                            }
                        }
                        match key_lower.as_str() {
                            "l" => {
                                ev.prevent_default();
                                if let Some((row, col)) = focused_cell_for_hotkey.get_untracked() {
                                    focus_grid_cell(row, col);
                                }
                            }
                            "p" => {
                                ev.prevent_default();
                                flush_mgr_for_hotkey.flush_all_then(move || {
                                    selected_monday_for_hotkey.set(
                                        selected_monday_for_hotkey.get_untracked()
                                            - Duration::weeks(1),
                                    );
                                });
                            }
                            "n" => {
                                ev.prevent_default();
                                flush_mgr_for_hotkey.flush_all_then(move || {
                                    selected_monday_for_hotkey.set(
                                        selected_monday_for_hotkey.get_untracked()
                                            + Duration::weeks(1),
                                    );
                                });
                            }
                            "d" => {
                                ev.prevent_default();
                                if let Some(window) = web_sys::window() {
                                    if let Some(document) = window.document() {
                                        if let Some(node) =
                                            document.query_selector(".nav-date").ok().flatten()
                                        {
                                            if let Some(el) = node.dyn_ref::<web_sys::HtmlElement>()
                                            {
                                                let _ = el.click();
                                            }
                                        }
                                    }
                                }
                            }
                            "t" => {
                                ev.prevent_default();
                                let today_monday = week_monday(today_for_hotkey.get_untracked());
                                if !today_is_visible(
                                    selected_monday_for_hotkey.get_untracked(),
                                    num_weeks.get_untracked(),
                                    today_for_hotkey.get_untracked(),
                                ) {
                                    flush_mgr_for_hotkey.flush_all_then(move || {
                                        selected_monday_for_hotkey.set(today_monday);
                                    });
                                }
                            }
                            "s" => {
                                ev.prevent_default();
                                flush_mgr_for_hotkey.flush_all();
                                show_settings.set(true);
                            }
                            "x" => {
                                ev.prevent_default();
                                if let Some(window) = web_sys::window() {
                                    let _ = window.location().set_href("/auth/logout");
                                }
                            }
                            "r" => {
                                ev.prevent_default();
                                show_report.set(true);
                            }
                            "f" => {
                                ev.prevent_default();
                                is_refreshing.set(true);
                                let flush_mgr = flush_mgr_for_hotkey.clone();
                                let data = data.clone();
                                let is_refreshing = is_refreshing.clone();
                                #[cfg(feature = "hydrate")]
                                leptos::task::spawn_local(async move {
                                    conn_for_hotkey.request_started();
                                    let _ = clear_cache().await;
                                    conn_for_hotkey.request_finished();
                                    flush_mgr.flush_all();
                                    data.refetch();
                                    is_refreshing.set(false);
                                });
                            }
                            _ => {}
                        }
                    }
                },
            );
        if let Some(window) = web_sys::window() {
            let _ = window.add_event_listener_with_callback(
                "keydown",
                focus_last_cell_cb.as_ref().unchecked_ref(),
            );
        }
        focus_last_cell_cb.forget();
    }

    view! {
    <div class=move || {
        match (show_report.get(), show_settings.get()) {
            (true, true) => "timesheet timesheet-report-open timesheet-settings-open",
            (true, false) => "timesheet timesheet-report-open",
            (false, true) => "timesheet timesheet-settings-open",
            (false, false) => "timesheet",
        }
    }>
        <Title text=move || {
            let name = user_name.get();
            let first_name = name.split_whitespace().next().unwrap_or("").to_string();
            format!("{} {}", i18n.get().t(keys::TIMESHEET_TITLE), first_name)
        } />

        <div class="ribbon">
            <div class="ribbon-section ribbon-left">
                {move || {
                    if show_report.get() {
                        let on_back = {
                            let show_report = show_report.clone();
                            move |_| show_report.set(false)
                        };
                        view! {
                            <button class="nav-btn nav-icon-btn nav-view-btn" on:click=on_back title=move || i18n.get().t(keys::TIMESHEET_TITLE)>
                                <svg viewBox="0 0 24 24" focusable="false" aria-hidden="true">
                                    <rect x="3" y="4" width="18" height="16" rx="2.2" fill="none" stroke="currentColor" stroke-width="1.6"></rect>
                                    <path d="M3 9h18M3 13.5h18M9 4v16M15 4v16" fill="none" stroke="currentColor" stroke-width="1.4" stroke-linecap="round"></path>
                                    <rect x="9.8" y="10.2" width="4.4" height="2.2" rx="0.5" fill="currentColor" opacity="0.35"></rect>
                                </svg>
                            </button>
                        }
                        .into_any()
                    } else {
                        view! {
                            <>
                                <button class="nav-btn nav-icon-btn nav-view-btn" on:click=on_open_report title=move || i18n.get().t(keys::USER_REPORT)>
                                    <svg viewBox="0 0 24 24" focusable="false" aria-hidden="true">
                                        <rect class="report-icon-bar report-icon-bar-left" x="4" y="10" width="4" height="10" rx="1"></rect>
                                        <rect class="report-icon-bar report-icon-bar-middle" x="10" y="6" width="4" height="14" rx="1"></rect>
                                        <rect class="report-icon-bar report-icon-bar-right" x="16" y="12" width="4" height="8" rx="1"></rect>
                                    </svg>
                                </button>
                                <button class="nav-btn nav-icon-btn nav-force-refresh" on:click=on_force_periodic_refresh title=move || i18n.get().t(keys::FORCE_PERIODIC_REFRESH) aria-label=move || i18n.get().t(keys::FORCE_PERIODIC_REFRESH)>
                                    <span class="icon-force-refresh">{"⟳"}</span>
                                </button>
                                <button class="nav-btn nav-icon-btn nav-refresh" on:click={
                                    let flush_mgr = flush_mgr_for_left.clone();
                                    let _conn = conn.clone();
                                    let _data = data.clone();
                                    let is_refreshing = is_refreshing.clone();
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
                                } disabled=move || is_refreshing.get() title=move || i18n.get().t(keys::REFRESH_CACHED)>
                                    <span class="icon-refresh">{"🔄"}</span>
                                </button>
                                {move || {
                                    let actions = custom_actions.get();
                                    if actions.is_empty() {
                                        return view! { <></> }.into_any();
                                    }
                                    view! {
                                        <>
                                            <span class="custom-action-separator" aria-hidden="true"></span>
                                            {actions
                                                .into_iter()
                                                .enumerate()
                                                .map(|(idx, action)| {
                                                    let title = custom_action_title(&action);
                                                    let on_click = {
                                                        let trigger_custom_action = trigger_custom_action;
                                                        move |_| trigger_custom_action.run(idx)
                                                    };
                                                    view! {
                                                        <button
                                                            class="nav-btn nav-icon-btn nav-custom-action"
                                                            on:click=on_click
                                                            title={title}
                                                            disabled=move || !conn.is_available()
                                                        >
                                                            <svg viewBox="0 0 24 24" focusable="false" aria-hidden="true">
                                                                <rect x="4" y="3.5" width="13.5" height="17" rx="2.2" fill="none" stroke="currentColor" stroke-width="1.5"></rect>
                                                                <path d="M7 8h8M7 12h8M7 16h6" fill="none" stroke="currentColor" stroke-width="1.4" stroke-linecap="round"></path>
                                                                <path d="M17.5 16.5l1.3 3.5 1.8-1.4 1.4 1.9 1.2-.9-1.4-1.9 2.2-.5-2.8-2.4z" fill="currentColor"></path>
                                                            </svg>
                                                            <span class="custom-action-index">{idx + 1}</span>
                                                        </button>
                                                    }
                                                })
                                                .collect_view()}
                                        </>
                                    }
                                    .into_any()
                                }}
                            </>
                        }
                        .into_any()
                    }
                }}
            </div>

            <div class="ribbon-section ribbon-center">
                {move || {
                    if show_report.get() {
                        view! { <ReportRibbonControls state=report_state_for_ribbon.clone() /> }.into_any()
                    } else {
                        view! { <WeekNavigator selected_monday=selected_monday tab_index_base=3 /> }.into_any()
                    }
                }}
            </div>

            <div class="ribbon-section ribbon-right">
                <div class="avatar-dropdown">
                    <button
                        class="avatar-btn"
                        on:click={
                            let user_menu_open = user_menu_open.clone();
                            move |_| user_menu_open.update(|open| *open = !*open)
                        }
                        aria-label=move || user_name.get()
                    >
                        <img src={move || user_avatar.get()} class="avatar-img" alt={move || user_name.get()} />
                    </button>
                    <div class=move || if user_menu_open.get() { "avatar-menu avatar-menu-open" } else { "avatar-menu" }>
                        <button class="avatar-menu-item" title=move || i18n.get().t(keys::OPEN_SETTINGS) on:click={
                            let flush_mgr = flush_mgr_for_right.clone();
                            move |_| {
                                flush_mgr.flush_all();
                                user_menu_open.set(false);
                                show_settings.set(true);
                            }
                        }>
                            <span aria-hidden="true">{"⚙"}</span>
                        </button>
                        <a class="avatar-menu-item" href="/auth/logout" title=move || i18n.get().t(keys::LOGOUT) on:click=move |_| user_menu_open.set(false)>
                            <span aria-hidden="true">{"🚪"}</span>
                        </a>
                    </div>
                </div>
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

        {move || (!show_report.get()).then(|| error_msg.get().map(|e| view! { <p class="error">{e}</p> })).flatten()}

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
                    let nav_row_count = ts.work_items.len();
                    let nav_col_count = nw * 6;

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
                        .enumerate()
                        .map(|(row_idx, item)| {
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
                                    let has_test_results = !cell_activity.test_result_links.is_empty();

                                    let has_commit_associations = !cell_activity.commit_messages.is_empty()
                                        || !cell_activity.commit_links.is_empty();
                                    let has_worklogs = !worklogs.is_empty();
                                    let show_corner_commit_overlay =
                                        has_worklogs && has_commit_associations;
                                    let show_corner_pr_overlay = has_worklogs && has_pr_review;
                                    let show_corner_test_overlay = has_worklogs && has_test_results;
                                    let show_center_dual_overlay =
                                        !has_worklogs && has_commit_associations && has_pr_review;
                                    let show_center_commit_overlay =
                                        !has_worklogs && has_commit_associations && !has_pr_review;
                                    let show_center_pr_overlay =
                                        !has_worklogs && !has_commit_associations && has_pr_review;
                                    let show_center_test_overlay =
                                        !has_worklogs && has_test_results;
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
                                    let nav_col = wi * 6 + di;
                                    let cell_summary = summary.clone();
                                    let site_url_for_cell = site_url.clone();
                                    let owner_for_cell_popup = component_owner.clone();
                                    let row_pr_links_for_cell = row_pr_links.clone();
                                    let open_weekday_popup: std::rc::Rc<dyn Fn(Option<PopupSeed>)> = std::rc::Rc::new({
                                        let ck2 = ck2.clone();
                                        let entries2 = entries2.clone();
                                        let bb_activity_for_closure = bb_activity_for_closure.clone();
                                        let cell_summary = cell_summary.clone();
                                        let row_pr_links_for_cell = row_pr_links_for_cell.clone();
                                        let site_url_for_cell = site_url_for_cell.clone();
                                        let owner_for_cell_popup = owner_for_cell_popup.clone();
                                        move |seed: Option<PopupSeed>| {
                                            let existing_popup_id = open_popups.with(|ps| {
                                                ps.iter()
                                                    .find(|p| p.issue_key == ck2 && p.date == cell_date)
                                                    .map(|p| p.popup_id)
                                            });
                                            if let Some(popup_id) = existing_popup_id {
                                                #[cfg(feature = "hydrate")]
                                                if let Some(seed) = seed {
                                                    match seed {
                                                        PopupSeed::Hours(d) => schedule_digit_fill_for_popup(popup_id, d),
                                                        PopupSeed::Description(ch) => schedule_description_fill_for_popup(popup_id, ch),
                                                    }
                                                }
                                                #[cfg(not(feature = "hydrate"))]
                                                let _ = popup_id;
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
                                                popup_id: NEXT_POPUP_ID.fetch_add(
                                                    1,
                                                    std::sync::atomic::Ordering::Relaxed,
                                                ),
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
                                                test_result_links: activity_links.test_result_links,
                                                pr_links: row_pr_links_for_cell.clone(),
                                                is_git_log,
                                                is_weekend: false,
                                                is_today: cell_is_today,
                                                position_style: owner_for_cell_popup
                                                    .with(|| RwSignal::new(pos_style)),
                                                site_url: site_url_for_cell.clone(),
                                                restored_timer_popup: None,
                                                initial_digit: match seed {
                                                    Some(PopupSeed::Hours(d)) => Some(d),
                                                    _ => None,
                                                },
                                            };
                                            let popup_id = popup.popup_id;
                                            open_popups.update(|ps| ps.push(popup));
                                            #[cfg(feature = "hydrate")]
                                            if let Some(PopupSeed::Description(ch)) = seed {
                                                schedule_description_fill_for_popup(popup_id, ch);
                                            }
                                            #[cfg(not(feature = "hydrate"))]
                                            let _ = popup_id;
                                        }
                                    });
                                    let open_weekday_popup_click = open_weekday_popup.clone();
                                    let open_weekday_popup_keydown = open_weekday_popup.clone();
                                    let focused_cell_for_focus = focused_cell;
                                    let focused_cell_for_tabindex = focused_cell;
                                    let focused_cell_for_keydown = focused_cell;
                                    week_cells.push(view! {
                                        <td class={cls} title={title}>
                                            <span
                                                class={if show_corner_commit_overlay
                                                    || show_corner_pr_overlay
                                                    || show_corner_test_overlay
                                                {
                                                    "cell-value cell-value-with-overlay-corners"
                                                } else if show_center_dual_overlay {
                                                    "cell-value cell-value-with-overlay-center-pair"
                                                } else if show_center_commit_overlay || show_center_test_overlay {
                                                    "cell-value cell-value-with-commit-overlay-center"
                                                } else {
                                                    "cell-value"
                                                }}
                                                data-cell-key={cell_key}
                                                data-cell-date={cell_date_str}
                                                data-nav-row={row_idx.to_string()}
                                                data-nav-col={nav_col.to_string()}
                                                role="button"
                                                tabindex={move || {
                                                    if focused_cell_for_tabindex.get() == Some((row_idx, nav_col)) {
                                                        1
                                                    } else {
                                                        -1
                                                    }
                                                }}
                                                on:focus=move |_| {
                                                    focused_cell_for_focus.set(Some((row_idx, nav_col)));
                                                }
                                                on:click=move |_| {
                                                    open_weekday_popup_click(None);
                                                }
                                                on:keydown=move |ev: leptos::ev::KeyboardEvent| {
                                                    let key = ev.key();
                                                    match key.as_str() {
                                                        "ArrowUp" | "ArrowDown" | "ArrowLeft" | "ArrowRight" => {
                                                            ev.prevent_default();
                                                            let row_count = nav_row_count;
                                                            let col_count = nav_col_count;
                                                            if row_count == 0 || col_count == 0 {
                                                                return;
                                                            }
                                                            let mut next_row = row_idx;
                                                            let mut next_col = nav_col;
                                                            match key.as_str() {
                                                                "ArrowUp" => {
                                                                    next_row = next_row.saturating_sub(1);
                                                                }
                                                                "ArrowDown" => {
                                                                    next_row = (next_row + 1).min(row_count - 1);
                                                                }
                                                                "ArrowLeft" => {
                                                                    next_col = next_col.saturating_sub(1);
                                                                }
                                                                "ArrowRight" => {
                                                                    next_col = (next_col + 1).min(col_count - 1);
                                                                }
                                                                _ => {}
                                                            }
                                                            focused_cell_for_keydown.set(Some((next_row, next_col)));
                                                            #[cfg(feature = "hydrate")]
                                                            focus_grid_cell(next_row, next_col);
                                                        }
                                                        "Enter" => {
                                                            ev.prevent_default();
                                                            open_weekday_popup_keydown(None);
                                                        }
                                                        _ => {
                                                            if key.len() == 1 {
                                                                if let Some(digit) = key.chars().next().filter(|c| {
                                                                    c.is_ascii_digit()
                                                                        && !ev.alt_key()
                                                                        && !ev.ctrl_key()
                                                                        && !ev.meta_key()
                                                                        && !ev.shift_key()
                                                                }) {
                                                                    ev.prevent_default();
                                                                    open_weekday_popup_keydown(Some(PopupSeed::Hours(digit)));
                                                                } else if let Some(letter) = key.chars().next().filter(|c| {
                                                                    c.is_ascii_alphabetic()
                                                                        && !ev.alt_key()
                                                                        && !ev.ctrl_key()
                                                                        && !ev.meta_key()
                                                                }) {
                                                                    ev.prevent_default();
                                                                    open_weekday_popup_keydown(Some(PopupSeed::Description(letter)));
                                                                }
                                                            }
                                                        }
                                                    }
                                                }
                                            >
                                                {if show_corner_commit_overlay
                                                    || show_corner_pr_overlay
                                                    || show_corner_test_overlay
                                                {
                                                    view! {
                                                        <>
                                                            {show_corner_commit_overlay.then(|| view! {
                                                                <span class="cell-commit-overlay-mark cell-commit-overlay-mark--corner-left">{"c"}</span>
                                                            })}
                                                            {show_corner_pr_overlay.then(|| view! {
                                                                <span class="cell-commit-overlay-mark cell-pr-overlay-mark--corner-right">{"p"}</span>
                                                            })}
                                                            {show_corner_test_overlay.then(|| view! {
                                                                <span class="cell-commit-overlay-mark cell-test-overlay-mark--corner-center">{"T"}</span>
                                                            })}
                                                            <span class="cell-value-text">{cell_display.clone()}</span>
                                                        </>
                                                    }
                                                        .into_any()
                                                } else if show_center_dual_overlay {
                                                    view! {
                                                        <>
                                                            <span class="cell-commit-overlay-mark cell-commit-overlay-mark--pair-left">{"c"}</span>
                                                            <span class="cell-commit-overlay-mark cell-pr-overlay-mark--pair-right">{"p"}</span>
                                                            {show_center_test_overlay.then(|| view! {
                                                                <span class="cell-commit-overlay-mark cell-test-overlay-mark--pair-center">{"T"}</span>
                                                            })}
                                                            <span class="cell-value-text"></span>
                                                        </>
                                                    }
                                                        .into_any()
                                                } else if show_center_commit_overlay {
                                                    view! {
                                                        <>
                                                            <span class="cell-commit-overlay-mark cell-commit-overlay-mark--center">{"c"}</span>
                                                            {show_center_test_overlay.then(|| view! {
                                                                <span class="cell-commit-overlay-mark cell-test-overlay-mark--pair-center">{"T"}</span>
                                                            })}
                                                            <span class="cell-value-text"></span>
                                                        </>
                                                    }
                                                        .into_any()
                                                } else if show_center_pr_overlay {
                                                    view! {
                                                        <>
                                                            <span class="cell-commit-overlay-mark cell-pr-overlay-mark--center">{"p"}</span>
                                                            {show_center_test_overlay.then(|| view! {
                                                                <span class="cell-commit-overlay-mark cell-test-overlay-mark--pair-center">{"T"}</span>
                                                            })}
                                                            <span class="cell-value-text"></span>
                                                        </>
                                                    }
                                                        .into_any()
                                                } else if show_center_test_overlay {
                                                    view! {
                                                        <>
                                                            <span class="cell-commit-overlay-mark cell-test-overlay-mark--center">{"T"}</span>
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
                                let weekend_has_test_results = !weekend_activity_sat
                                    .test_result_links
                                    .is_empty()
                                    || !weekend_activity_sun.test_result_links.is_empty();
                                let weekend_has_commit_associations = !weekend_commit_messages.is_empty()
                                    || !weekend_activity_sat.commit_links.is_empty()
                                    || !weekend_activity_sun.commit_links.is_empty();
                                let weekend_has_worklogs = !we_entries.is_empty();
                                let show_corner_commit_overlay_weekend =
                                    weekend_has_worklogs && weekend_has_commit_associations;
                                let show_corner_pr_overlay_weekend =
                                    weekend_has_worklogs && weekend_has_pr_review;
                                let show_corner_test_overlay_weekend =
                                    weekend_has_worklogs && weekend_has_test_results;
                                let show_center_dual_overlay_weekend = !weekend_has_worklogs
                                    && weekend_has_commit_associations
                                    && weekend_has_pr_review;
                                let show_center_commit_overlay_weekend = !weekend_has_worklogs
                                    && weekend_has_commit_associations
                                    && !weekend_has_pr_review;
                                let show_center_pr_overlay_weekend = !weekend_has_worklogs
                                    && !weekend_has_commit_associations
                                    && weekend_has_pr_review;
                                let show_center_test_overlay_weekend =
                                    !weekend_has_worklogs && weekend_has_test_results;
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
                                let weekend_nav_col = wi * 6 + 5;
                                let we_summary = summary.clone();
                                let site_url_for_we = site_url.clone();
                                let owner_for_weekend_popup = component_owner.clone();
                                let row_pr_links_for_weekend = row_pr_links.clone();
                                let open_weekend_popup: std::rc::Rc<dyn Fn(Option<PopupSeed>)> = std::rc::Rc::new({
                                    let we_key2 = we_key2.clone();
                                    let we_entries2 = we_entries2.clone();
                                    let bb_activity_for_closure = bb_activity_for_closure.clone();
                                    let we_summary = we_summary.clone();
                                    let row_pr_links_for_weekend = row_pr_links_for_weekend.clone();
                                    let site_url_for_we = site_url_for_we.clone();
                                    let owner_for_weekend_popup = owner_for_weekend_popup.clone();
                                    move |seed: Option<PopupSeed>| {
                                        let existing_popup_id = open_popups.with(|ps| {
                                            ps.iter()
                                                .find(|p| p.issue_key == we_key2 && p.date == sat)
                                                .map(|p| p.popup_id)
                                        });
                                        if let Some(popup_id) = existing_popup_id {
                                            #[cfg(feature = "hydrate")]
                                            if let Some(seed) = seed {
                                                match seed {
                                                    PopupSeed::Hours(d) => schedule_digit_fill_for_popup(popup_id, d),
                                                    PopupSeed::Description(ch) => schedule_description_fill_for_popup(popup_id, ch),
                                                }
                                            }
                                            #[cfg(not(feature = "hydrate"))]
                                            let _ = popup_id;
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
                                            popup_id: NEXT_POPUP_ID
                                                .fetch_add(1, std::sync::atomic::Ordering::Relaxed),
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
                                            test_result_links: {
                                                let mut links = sat_links.test_result_links;
                                                links.extend(sun_links.test_result_links);
                                                let mut seen = std::collections::HashSet::new();
                                                links.retain(|link| seen.insert(link.clone()));
                                                links
                                            },
                                            pr_links: row_pr_links_for_weekend.clone(),
                                            is_git_log,
                                            is_weekend: true,
                                            is_today: we_cell_is_today,
                                            position_style: owner_for_weekend_popup
                                                .with(|| RwSignal::new(pos_style)),
                                            site_url: site_url_for_we.clone(),
                                            restored_timer_popup: None,
                                            initial_digit: match seed {
                                                Some(PopupSeed::Hours(d)) => Some(d),
                                                _ => None,
                                            },
                                        };
                                        let popup_id = popup.popup_id;
                                        open_popups.update(|ps| ps.push(popup));
                                        #[cfg(feature = "hydrate")]
                                        if let Some(PopupSeed::Description(ch)) = seed {
                                            schedule_description_fill_for_popup(popup_id, ch);
                                        }
                                        #[cfg(not(feature = "hydrate"))]
                                        let _ = popup_id;
                                    }
                                });
                                let open_weekend_popup_click = open_weekend_popup.clone();
                                let open_weekend_popup_keydown = open_weekend_popup.clone();
                                let focused_cell_for_weekend_focus = focused_cell;
                                let focused_cell_for_weekend_tabindex = focused_cell;
                                let focused_cell_for_weekend_keydown = focused_cell;
                                week_cells.push(view! {
                                    <td class={weekend_cls} title={we_tooltip}>
                                        <span
                                            class={if show_corner_commit_overlay_weekend
                                                || show_corner_pr_overlay_weekend
                                                || show_corner_test_overlay_weekend
                                            {
                                                "cell-value cell-value-with-overlay-corners"
                                            } else if show_center_dual_overlay_weekend {
                                                "cell-value cell-value-with-overlay-center-pair"
                                            } else if show_center_commit_overlay_weekend
                                                || show_center_pr_overlay_weekend
                                                || show_center_test_overlay_weekend
                                            {
                                                "cell-value cell-value-with-commit-overlay-center"
                                            } else {
                                                "cell-value"
                                            }}
                                            data-cell-key={we_key}
                                            data-cell-date={we_sat_str}
                                            data-nav-row={row_idx.to_string()}
                                            data-nav-col={weekend_nav_col.to_string()}
                                            role="button"
                                            tabindex={move || {
                                                if focused_cell_for_weekend_tabindex.get()
                                                    == Some((row_idx, weekend_nav_col))
                                                {
                                                    1
                                                } else {
                                                    -1
                                                }
                                            }}
                                            on:focus=move |_| {
                                                focused_cell_for_weekend_focus
                                                    .set(Some((row_idx, weekend_nav_col)));
                                            }
                                            on:click=move |_| {
                                                open_weekend_popup_click(None);
                                            }
                                            on:keydown=move |ev: leptos::ev::KeyboardEvent| {
                                                let key = ev.key();
                                                match key.as_str() {
                                                    "ArrowUp" | "ArrowDown" | "ArrowLeft" | "ArrowRight" => {
                                                        ev.prevent_default();
                                                        let row_count = nav_row_count;
                                                        let col_count = nav_col_count;
                                                        if row_count == 0 || col_count == 0 {
                                                            return;
                                                        }
                                                        let mut next_row = row_idx;
                                                        let mut next_col = weekend_nav_col;
                                                        match key.as_str() {
                                                            "ArrowUp" => {
                                                                next_row = next_row.saturating_sub(1);
                                                            }
                                                            "ArrowDown" => {
                                                                next_row = (next_row + 1).min(row_count - 1);
                                                            }
                                                            "ArrowLeft" => {
                                                                next_col = next_col.saturating_sub(1);
                                                            }
                                                            "ArrowRight" => {
                                                                next_col = (next_col + 1).min(col_count - 1);
                                                            }
                                                            _ => {}
                                                        }
                                                        focused_cell_for_weekend_keydown
                                                            .set(Some((next_row, next_col)));
                                                        #[cfg(feature = "hydrate")]
                                                        focus_grid_cell(next_row, next_col);
                                                    }
                                                    "Enter" => {
                                                        ev.prevent_default();
                                                        open_weekend_popup_keydown(None);
                                                    }
                                                    _ => {
                                                        if key.len() == 1 {
                                                            if let Some(digit) = key.chars().next().filter(|c| {
                                                                c.is_ascii_digit()
                                                                    && !ev.alt_key()
                                                                    && !ev.ctrl_key()
                                                                    && !ev.meta_key()
                                                                    && !ev.shift_key()
                                                            }) {
                                                                ev.prevent_default();
                                                                open_weekend_popup_keydown(Some(PopupSeed::Hours(digit)));
                                                            } else if let Some(letter) = key.chars().next().filter(|c| {
                                                                c.is_ascii_alphabetic()
                                                                    && !ev.alt_key()
                                                                    && !ev.ctrl_key()
                                                                    && !ev.meta_key()
                                                            }) {
                                                                ev.prevent_default();
                                                                open_weekend_popup_keydown(Some(PopupSeed::Description(letter)));
                                                            }
                                                        }
                                                    }
                                                }
                                            }

                                        >
                                            {if show_corner_commit_overlay_weekend
                                                || show_corner_pr_overlay_weekend
                                                || show_corner_test_overlay_weekend
                                            {
                                                view! {
                                                    <>
                                                        {show_corner_commit_overlay_weekend.then(|| view! {
                                                            <span class="cell-commit-overlay-mark cell-commit-overlay-mark--corner-left">{"c"}</span>
                                                        })}
                                                        {show_corner_pr_overlay_weekend.then(|| view! {
                                                            <span class="cell-commit-overlay-mark cell-pr-overlay-mark--corner-right">{"p"}</span>
                                                        })}
                                                        {show_corner_test_overlay_weekend.then(|| view! {
                                                            <span class="cell-commit-overlay-mark cell-test-overlay-mark--corner-center">{"T"}</span>
                                                        })}
                                                        <span class="cell-value-text">{we_display.clone()}</span>
                                                    </>
                                                }
                                                    .into_any()
                                            } else if show_center_dual_overlay_weekend {
                                                view! {
                                                    <>
                                                        <span class="cell-commit-overlay-mark cell-commit-overlay-mark--pair-left">{"c"}</span>
                                                        <span class="cell-commit-overlay-mark cell-pr-overlay-mark--pair-right">{"p"}</span>
                                                        {show_center_test_overlay_weekend.then(|| view! {
                                                            <span class="cell-commit-overlay-mark cell-test-overlay-mark--pair-center">{"T"}</span>
                                                        })}
                                                        <span class="cell-value-text"></span>
                                                    </>
                                                }
                                                    .into_any()
                                            } else if show_center_commit_overlay_weekend {
                                                view! {
                                                    <>
                                                        <span class="cell-commit-overlay-mark cell-commit-overlay-mark--center">{"c"}</span>
                                                        {show_center_test_overlay_weekend.then(|| view! {
                                                            <span class="cell-commit-overlay-mark cell-test-overlay-mark--pair-center">{"T"}</span>
                                                        })}
                                                        <span class="cell-value-text"></span>
                                                    </>
                                                }
                                                    .into_any()
                                            } else if show_center_pr_overlay_weekend {
                                                view! {
                                                    <>
                                                        <span class="cell-commit-overlay-mark cell-pr-overlay-mark--center">{"p"}</span>
                                                        {show_center_test_overlay_weekend.then(|| view! {
                                                            <span class="cell-commit-overlay-mark cell-test-overlay-mark--pair-center">{"T"}</span>
                                                        })}
                                                        <span class="cell-value-text"></span>
                                                    </>
                                                }
                                                    .into_any()
                                            } else if show_center_test_overlay_weekend {
                                                view! {
                                                    <>
                                                        <span class="cell-commit-overlay-mark cell-test-overlay-mark--center">{"T"}</span>
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
                                            tabindex="2"
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

            {move || {
                let toasts = refresh_toasts.get();
                if toasts.is_empty() {
                    return None;
                }
                let (offset_x, offset_y) = toast_stack_offset.get();
                let stack_style = format!("transform: translate({:.0}px, {:.0}px);", offset_x, offset_y);
                Some(view! {
                    <div class="refresh-toast-stack" style={stack_style} role="status" aria-live="polite">
                        {toasts.into_iter().map(|toast| {
                            let toast_id = toast.id;
                            let on_close = {
                                let refresh_toasts = refresh_toasts;
                                move |_| {
                                    refresh_toasts.update(|items| {
                                        items.retain(|item| item.id != toast_id);
                                    });
                                }
                            };
                            let on_drag_start = {
                                let toast_stack_offset = toast_stack_offset;
                                move |ev| attach_toast_stack_drag(ev, toast_stack_offset)
                            };
                            view! {
                                <article class="refresh-toast-card">
                                    <div class="refresh-toast-titlebar" on:mousedown=on_drag_start>
                                        <span class="refresh-toast-time">{toast.hhmm.clone()}</span>
                                        <button
                                            type="button"
                                            class="refresh-toast-close"
                                            aria-label={move || i18n.get().t(keys::LIVE_REFRESH_TOAST_CLOSE)}
                                            title={move || i18n.get().t(keys::LIVE_REFRESH_TOAST_CLOSE)}
                                            on:click=on_close
                                        >
                                            "×"
                                        </button>
                                    </div>
                                    <div class="refresh-toast-body">
                                        {(!toast.added_work_keys.is_empty()).then(|| {
                                            view! {
                                                <div class="refresh-toast-section">
                                                    <div class="refresh-toast-section-title">{move || i18n.get().t(keys::LIVE_REFRESH_WORK_KEYS_ADDED)}</div>
                                                    <div class="refresh-toast-keys">{toast.added_work_keys.join(", ")}</div>
                                                </div>
                                            }
                                        })}
                                        {(!toast.pr_updates.is_empty()).then(|| {
                                            view! {
                                                <div class="refresh-toast-section">
                                                    <div class="refresh-toast-section-title">{move || i18n.get().t(keys::LIVE_REFRESH_PR_UPDATES)}</div>
                                                    <ul class="refresh-toast-list">
                                                        {toast.pr_updates.iter().map(|update| {
                                                            let links = update.pr_links.clone();
                                                            view! {
                                                                <li>
                                                                    <span class="refresh-toast-key">{update.issue_key.clone()}</span>
                                                                    {(!links.is_empty()).then(|| view! {
                                                                        <div class="refresh-toast-links">
                                                                            {links.into_iter().map(|link| {
                                                                                view! {
                                                                                    <a href={link.clone()} target="_blank" rel="noopener noreferrer">{pr_number(link.as_str()).unwrap_or_default().to_string()}</a>
                                                                                }
                                                                            }).collect_view()}
                                                                        </div>
                                                                    })}
                                                                </li>
                                                            }
                                                        }).collect_view()}
                                                    </ul>
                                                </div>
                                            }
                                        })}
                                        {(!toast.test_updates.is_empty()).then(|| {
                                            view! {
                                                <div class="refresh-toast-section">
                                                    <div class="refresh-toast-section-title">{move || i18n.get().t(keys::LIVE_REFRESH_TEST_UPDATES)}</div>
                                                    <ul class="refresh-toast-list">
                                                        {toast.test_updates.iter().map(|update| {
                                                            let links = update.test_result_links.clone();
                                                            view! {
                                                                <li>
                                                                    <span class="refresh-toast-key">{update.issue_key.clone()}</span>
                                                                    {(!links.is_empty()).then(|| view! {
                                                                        <div class="refresh-toast-links">
                                                                            {links.into_iter().map(|link| {
                                                                                view! {
                                                                                    <a href={link.clone()} target="_blank" rel="noopener noreferrer">{link.clone()}</a>
                                                                                }
                                                                            }).collect_view()}
                                                                        </div>
                                                                    })}
                                                                </li>
                                                            }
                                                        }).collect_view()}
                                                    </ul>
                                                </div>
                                            }
                                        })}
                                    </div>
                                </article>
                            }
                        }).collect_view()}
                    </div>
                })
            }}

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
                        #[cfg(feature = "hydrate")]
                        if let Some((row, col)) = focused_cell.get_untracked() {
                            focus_grid_cell(row, col);
                        }
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
                           test_result_links={info.test_result_links.clone()}
                           pr_links={info.pr_links.clone()}
                           is_git_log={info.is_git_log}
                           is_weekend={info.is_weekend}
                           restored_timer_popup={info.restored_timer_popup.clone()}
                           is_today={info.is_today}
                           on_close=on_close_popup
                           on_changed=on_popup_changed
                           site_url={info.site_url}
                           initial_digit={info.initial_digit}
                        />
                    }
                }}
            />
            // Settings dialog modal
            {move || show_settings.get().then(|| view! {
                <SettingsDialog on_ok=on_settings_saved on_cancel=on_close_settings />
            })}
            {move || show_report.get().then(|| view! {
                <ReportView
                    state={report_state_for_view.clone()}
                    hours_per_day={last_data.get().map(|d| d.hours_per_day).unwrap_or(8.0)}
                    hours_per_week={last_data.get().map(|d| d.hours_per_week).unwrap_or(40.0)}
                />
            })}
        </div>
    }
}
