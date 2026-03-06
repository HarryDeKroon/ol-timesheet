use crate::components::cell_popup::CellPopup;
use crate::components::popup_flush::{provide_popup_flush_context, use_popup_flush};
use crate::components::settings_dialog::SettingsDialog;
use crate::components::timer::provide_timer_context;
use crate::components::week_navigator::{WeekNavigator, week_monday};
use crate::connection::use_connection;
use crate::formatting::{format_hours_long, format_hours_short};
use crate::i18n::{I18n, keys};
use crate::model::{ConnectionStatus, TimesheetData, WorkItem, WorklogEntry};
use chrono::{Datelike, Duration, Local, NaiveDate};
use leptos::prelude::*;

#[cfg(feature = "ssr")]
use crate::api::git::fetch_git_commits;

#[cfg(feature = "hydrate")]
use crate::api::git::check_for_new_git_commits;

// Import flag SVGs from shared flags module
use crate::flags::{FLAG_FR, FLAG_NL, FLAG_UK};

#[cfg(feature = "ssr")]
use std::collections::HashMap;

#[server(GetTimesheetData, "/api")]
pub async fn get_timesheet_data(
    start: NaiveDate,
    end: NaiveDate,
) -> Result<(TimesheetData, Option<(String, String)>), ServerFnError> {
    use crate::api::jira::timesheet_data_cache_key;

    let cache_key = timesheet_data_cache_key(start, end);

    // Check assembled-data cache first — this makes revisiting a week instant.
    if let Some(cached_json) = crate::api::cache::get(&cache_key) {
        if let Ok(ts) = serde_json::from_str::<TimesheetData>(&cached_json) {
            log::info!("[get_timesheet_data] cache hit for {} .. {}", start, end);
            // Still prefetch neighbours so the next navigation is instant.
            let selected_monday = end - chrono::Duration::days(6);
            let num_weeks = (((end - start).num_days() + 1) / 7).max(1) as usize;
            tokio::spawn(crate::api::jira::prefetch_adjacent_weeks(
                selected_monday,
                num_weeks,
            ));
            // Return a tuple with None for user_profile (since we don't cache it)
            return Ok((ts, None));
        }
    }

    let settings = crate::model::load_settings();

    // 1. Fetch issues from Jira (with worklogs in date range)
    let jira_items = crate::api::jira::fetch_work_items(&settings, start, end)
        .await
        .map_err(|e| ServerFnError::new(e))?;

    // 1b. Bitbucket PR integration is disabled due to API deprecation.
    let mut all_items = jira_items;

    // 2. Fetch worklogs for all issues (per-issue cache handles dedup)
    let mut all_worklogs = Vec::new();
    let mut ytd_hours: HashMap<String, f64> = HashMap::new();
    for item in &all_items {
        match crate::api::jira::fetch_worklogs(&settings, &item.key, start, end).await {
            Ok((wls, total)) => {
                all_worklogs.extend(wls);
                ytd_hours.insert(item.key.clone(), total);
            }
            Err(e) => log::warn!("Failed to fetch worklogs for {}: {}", item.key, e),
        }
    }

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

    // --- Git commit integration ---
    #[cfg(feature = "ssr")]
    let git_commits = {
        use std::collections::HashSet;
        let git_folder = settings.git_folder.clone();
        let work_item_keys: HashSet<_> = all_items.iter().map(|item| item.key.clone()).collect();
        // Collect user identifiers for matching git commit authors.
        let mut users: HashSet<String> = HashSet::new();
        if !settings.email.is_empty() {
            users.insert(settings.email.clone());
        }
        match fetch_git_commits(&git_folder, &work_item_keys, &users, start, end) {
            Ok(map) => Some(map),
            Err(e) => {
                log::warn!("[get_timesheet_data] fetch_git_commits failed: {}", e);
                None
            }
        }
    };

    #[cfg(feature = "ssr")]
    log::info!("git_commits: {git_commits:?}");

    #[cfg(not(feature = "ssr"))]
    let git_commits = None;

    let ts = TimesheetData {
        work_items: all_items,
        worklogs: all_worklogs,
        hours_per_week: settings.hours_per_week,
        hours_per_day: settings.hours_per_day,
        ytd_hours,
        git_commits,
        ..Default::default()
    };

    // Fetch Jira user profile (avatar and display name)
    let user_profile = match crate::api::jira::fetch_jira_user_profile(&settings).await {
        Ok(profile) => Some((profile.avatar_urls.size_48, profile.display_name)),
        Err(e) => {
            log::warn!("Failed to fetch Jira user profile: {}", e);
            None
        }
    };

    // Cache the assembled result so the same week is instant next time.
    if let Ok(json) = serde_json::to_string(&ts) {
        crate::api::cache::put(cache_key.clone(), json);
    }

    // Prefetch adjacent weeks in the background so the next navigation
    // is instant.  This never blocks the current response.
    let selected_monday = end - chrono::Duration::days(6);
    let num_weeks = (((end - start).num_days() + 1) / 7).max(1) as usize;
    tokio::spawn(crate::api::jira::prefetch_adjacent_weeks(
        selected_monday,
        num_weeks,
    ));

    Ok((ts, user_profile))
}

#[server(ClearCache, "/api")]
pub async fn clear_cache() -> Result<(), ServerFnError> {
    log::info!("[clear_cache] clearing all cached work items");
    crate::api::cache::clear_all();
    Ok(())
}

#[server(SearchWorkItems, "/api")]
pub async fn search_work_items(query: String) -> Result<Vec<WorkItem>, ServerFnError> {
    let settings = crate::model::load_settings();
    let items = crate::api::jira::search_issues(&settings, &query, 12)
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
    suggested_comment: Option<String>,
    is_git_log: bool,
    /// Whether the popup's date column is "today" (enables timer controls).
    is_today: bool,
    /// Inline CSS position computed at open time; updated by dragging.
    position_style: RwSignal<String>,
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
            suggested_comment: self.suggested_comment.clone(),
            is_git_log: self.is_git_log,
            is_today: self.is_today,
            position_style: self.position_style.clone(),
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
    let i18n = use_context::<RwSignal<I18n>>().expect("I18n context");

    // ── Timer context ──
    provide_timer_context();

    // ── Popup flush context ──
    provide_popup_flush_context();
    let flush_mgr = use_popup_flush();

    // --- Settings signal (assume loaded at app start, or fetch here if needed) ---
    // If you already have a settings signal/context, use that instead.
    // On the client, we use a default poll interval (5) for git polling.

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

    // --- Polling for new git commits ---
    #[cfg(not(feature = "ssr"))]
    {
        use wasm_bindgen::JsCast;
        use wasm_bindgen::closure::Closure;

        // State for showing the settings dialog
        let show_settings = RwSignal::new(false);

        let last_data = last_data.clone();
        let open_popups_poll = open_popups;
        let show_settings = show_settings.clone();
        let selected_monday = selected_monday.clone();
        let num_weeks = num_weeks.clone();

        Effect::new(move |_| {
            let poll_interval = 5u32;
            let poll_interval_ms = poll_interval * 60 * 1_000;

            let closure = Closure::wrap(Box::new(move || {
                log::info!("Checking for new commits.");
                if open_popups_poll.with_untracked(|p| p.is_empty())
                    && !show_settings.get_untracked()
                {
                    let known_keys: Vec<String> = last_data
                        .get_untracked()
                        .as_ref()
                        .map(|ts| ts.work_items.iter().map(|wi| wi.key.clone()).collect())
                        .unwrap_or_else(Vec::new);

                    let monday = selected_monday.get_untracked();
                    let nw = num_weeks.get_untracked();
                    let start = monday - chrono::Duration::weeks((nw as i64) - 1);
                    let end = monday + chrono::Duration::days(6);

                    #[cfg(feature = "hydrate")]
                    leptos::task::spawn_local(async move {
                        if let Ok(new_items) =
                            check_for_new_git_commits(known_keys.clone(), start, end).await
                        {
                            if !new_items.is_empty() {
                                last_data.update(|opt| {
                                    if let Some(ts) = opt {
                                        for (key, summary) in new_items.iter() {
                                            if !ts.work_items.iter().any(|wi| &wi.key == key) {
                                                ts.work_items.insert(
                                                    0,
                                                    WorkItem {
                                                        key: key.clone(),
                                                        summary: summary.clone(),
                                                        icon_url: String::new(),
                                                        issue_type: String::from("Git"),
                                                    },
                                                );
                                            }
                                        }
                                    }
                                });
                            }
                        }
                    });
                }
            }) as Box<dyn Fn()>);

            if let Some(window) = web_sys::window() {
                window
                    .set_interval_with_callback_and_timeout_and_arguments_0(
                        closure.as_ref().unchecked_ref(),
                        poll_interval_ms as i32,
                    )
                    .expect("failed to set interval");
            }
            closure.forget();
        });
    }

    // State for showing the settings dialog
    let show_settings = RwSignal::new(false);

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

    let on_popup_changed = Callback::new(move |_: ()| {
        data.refetch();
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
                web_sys::window()
                    .unwrap()
                    .set_timeout_with_callback_and_timeout_and_arguments_0(&resolve, 300)
                    .unwrap();
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
                                if s.is_empty() { String::new() } else { format!("[{}]", s) }
                            };

                            let icon_url = item.icon_url.clone();
                            let summary = item.summary.clone();
                            let _summary_for_title = summary.clone();
                            let key_display = key.clone();

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

                                    let git_commits_for_closure = ts.git_commits.clone();
                                    let git_commit_msgs = git_commits_for_closure.as_ref().and_then(|map| map.get(&format!("{}:{}", key, d)));

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
                                    } else if let Some(msgs) = git_commit_msgs {
                                        // Show ? for git commit, with tooltip
                                        ("?".to_string(), msgs.join("\n"))
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

                                    let suggested_comment = if title.is_empty() { None } else { Some(title.clone()) };
                                    let cell_is_today = is_today;
                                    let cell_summary = summary.clone();
                                    week_cells.push(view! {
                                        <td class={cls} title={title}>
                                            <span
                                                class="cell-value"
                                                data-cell-key={cell_key}
                                                data-cell-date={cell_date_str}
                                                on:click=move |_| {
                                                    if !conn.is_available() {
                                                        return;
                                                    }
                                                    let is_git_log = entries2.is_empty() && git_commits_for_closure.as_ref().and_then(|map| map.get(&format!("{}:{}", ck2, cell_date))).is_some();
                                                    // Don't open a duplicate popup for the same cell.
                                                    let already_open = open_popups.with(|ps| ps.iter().any(|p| p.issue_key == ck2 && p.date == cell_date));
                                                    if already_open {
                                                        return;
                                                    }
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
                                                        suggested_comment: suggested_comment.clone(),
                                                        is_git_log,
                                                        is_today: cell_is_today,
                                                        position_style: RwSignal::new(pos_style),
                                                    };
                                                    open_popups.update(|ps| ps.push(popup));
                                                }
                                            >
                                                {cell_display}
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

                                let we_key = key.clone();

                                let we_sat_str = sat.to_string();

                                let we_key2 = we_key.clone();

                                let we_entries2 = we_entries.clone();
                                let git_commits_for_closure = ts.git_commits.clone();


                                let suggested_comment = if we_title.is_empty() { None } else { Some(we_title.clone()) };
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
                                week_cells.push(view! {
                                    <td class={weekend_cls} title={we_title}>
                                        <span
                                            class="cell-value"
                                            data-cell-key={we_key}
                                            data-cell-date={we_sat_str}

                                            on:click=move |_| {
                                                if !conn.is_available() {
                                                    return;
                                                }
                                                let is_git_log = we_entries2.is_empty() && git_commits_for_closure.as_ref().and_then(|map| map.get(&format!("{}:{}", we_key2, sat))).is_some();
                                                let already_open = open_popups.with(|ps| ps.iter().any(|p| p.issue_key == we_key2 && p.date == sat));
                                                if already_open {
                                                    return;
                                                }
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
                                                    suggested_comment: suggested_comment.clone(),
                                                    is_git_log,
                                                    is_today: we_cell_is_today,
                                                    position_style: RwSignal::new(pos_style),
                                                };
                                                open_popups.update(|ps| ps.push(popup));
                                            }

                                        >
                                            {we_text}
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
                                        <span class="issue-total">{header_total}</span>
                                        <span class="issue-summary">{summary}</span>
                                    </td>
                                    {week_cells}
                                </tr>
                            }.into_any()
                        })
                        .collect();

                    view! {
                        <div class="timesheet-table-wrap">
                            <table class="timesheet-grid">
                                <thead>
                                    <tr>
                                        <th class="col-item">
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
                           suggested_comment={info.suggested_comment.clone()}
                           is_git_log={info.is_git_log}
                           is_today={info.is_today}
                           on_close=on_close_popup
                           on_changed=on_popup_changed
                        />
                    }
                }}
            />
            // Settings dialog modal
            {move || show_settings.get().then(|| view! {
                <SettingsDialog on_ok=on_close_settings on_cancel=on_close_settings />
            })}
        </div>
    }
}
