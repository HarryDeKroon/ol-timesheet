use crate::components::popup_flush::{FlushLatch, use_popup_flush};
use crate::components::timer::{
    PersistedTimerPhase, PersistedTimerPopup, PersistedTimerRow, PersistedTimerState, ProgressInfo,
    TimerId, TimerPhase, ensure_timer_storage_initialized, remove_persisted_timer_popup,
    save_persisted_timer_popup, upsert_persisted_timer_row, use_timer,
};
use crate::connection::use_connection;
use crate::formatting::{format_hours_long, parse_hours};
use crate::i18n::{I18n, keys};
use crate::model::WorklogEntry;
use chrono::NaiveDate;
use leptos::prelude::*;

use std::cell::Cell;
use std::collections::{HashMap, HashSet};
use std::rc::Rc;
use std::sync::{Arc, atomic::AtomicBool};

/// Generate a replay-protection nonce in `{unix_secs}:{random_hex}` format.
/// On WASM uses `js_sys`; on the server side (SSR compilation stub) uses chrono.

/// Monotonically increasing counter used to assign z-index to popups when they
/// gain focus, so the most recently focused popup is always on top.
static POPUP_Z_COUNTER: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(100);

/// Parse `"left:Xpx;top:Ypx[;...]"` into `(left, top)` in pixels.
fn parse_popup_pos(style: &str) -> (f64, f64) {
    let mut left = 0.0_f64;
    let mut top = 0.0_f64;
    for part in style.split(';') {
        let part = part.trim();
        if let Some(v) = part.strip_prefix("left:") {
            left = v.trim_end_matches("px").parse().unwrap_or(0.0);
        } else if let Some(v) = part.strip_prefix("top:") {
            top = v.trim_end_matches("px").parse().unwrap_or(0.0);
        }
    }
    (left, top)
}
fn new_request_nonce() -> String {
    cfg_if::cfg_if! {
        if #[cfg(feature = "ssr")] {
            // Compiled for SSR but never called at runtime from this path.
            format!("{}:{}", chrono::Utc::now().timestamp(), uuid::Uuid::new_v4())
        } else {
            let ts = (js_sys::Date::now() / 1000.0) as i64;
            let r1 = (js_sys::Math::random() * f64::from(u32::MAX)) as u32;
            let r2 = (js_sys::Math::random() * f64::from(u32::MAX)) as u32;
            format!("{}:{:08x}{:08x}", ts, r1, r2)
        }
    }
}

#[server(AddWorklog, "/api")]
pub async fn server_add_worklog(
    issue_key: String,
    date: NaiveDate,
    hours: f64,
    comment: String,
    request_nonce: String,
) -> Result<(), ServerFnError> {
    let (session_id, session) = crate::auth::current_user_session().await?;
    crate::auth::validate_nonce(&session_id, &request_nonce)?;
    let creds = session.jira_credentials();
    crate::api::jira::add_worklog(&creds, &issue_key, date, hours, &comment)
        .await
        .map(|_| ())
        .map_err(|e| ServerFnError::new(e))
}

#[server(UpdateWorklog, "/api")]
pub async fn server_update_worklog(
    issue_key: String,
    worklog_id: String,
    hours: f64,
    comment: String,
    comment_adf: Option<String>,
    request_nonce: String,
) -> Result<(), ServerFnError> {
    let (session_id, session) = crate::auth::current_user_session().await?;
    crate::auth::validate_nonce(&session_id, &request_nonce)?;
    let creds = session.jira_credentials();
    crate::api::jira::update_worklog(
        &creds,
        &issue_key,
        &worklog_id,
        hours,
        &comment,
        comment_adf.as_deref(),
    )
    .await
    .map_err(|e| ServerFnError::new(e))
}

#[server(DeleteWorklog, "/api")]
pub async fn server_delete_worklog(
    issue_key: String,
    worklog_id: String,
    request_nonce: String,
) -> Result<(), ServerFnError> {
    let (session_id, session) = crate::auth::current_user_session().await?;
    crate::auth::validate_nonce(&session_id, &request_nonce)?;
    let creds = session.jira_credentials();
    crate::api::jira::delete_worklog(&creds, &issue_key, &worklog_id)
        .await
        .map_err(|e| ServerFnError::new(e))
}

/// Build a Jira URL that deep-links to a specific worklog entry.
fn worklog_url(site_url: &str, issue_key: &str, worklog_id: &str) -> String {
    format!(
        "{}/browse/{}?focusedWorklogId={}",
        site_url.trim_end_matches('/'),
        issue_key,
        worklog_id,
    )
}

#[cfg(feature = "hydrate")]
fn schedule_initial_digit_fill(popup_id: u32, digit: char) {
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

/// Data kept per existing worklog entry in the popup.
#[derive(Clone)]
struct ExistingEntry {
    id: String,
    hours_sig: RwSignal<String>,
    /// The initial formatted hours text (captured at popup open time).
    /// Used for dirty detection so flush-on-navigate only saves when
    /// the user actually changed something.
    initial_hours: String,
    /// Plain-text comment signal — only used when there is no ADF preview.
    comment_sig: RwSignal<String>,
    /// The initial display text (whitespace-collapsed). Used for change
    /// detection so we know whether to preserve the original ADF.
    display_comment: String,
    /// HTML rendering of the ADF comment.  When non-empty the popup shows
    /// a read-only rich preview instead of a text input.
    comment_html: String,
    /// Raw ADF JSON for round-tripping unchanged comments.
    comment_adf: Option<String>,
    /// Whether this entry has been marked for deletion (deferred until Save).
    deleted: RwSignal<bool>,
}

#[derive(Clone, Default, PartialEq)]
struct PopupRowValidation {
    hours_error: Option<String>,
    description_error: Option<String>,
}

impl PopupRowValidation {
    fn is_valid(&self) -> bool {
        self.hours_error.is_none() && self.description_error.is_none()
    }
}

fn normalize_popup_description(text: &str) -> String {
    text.split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase()
}

#[component]
pub fn CellPopup(
    popup_id: u32,
    pos_sig: RwSignal<String>,
    issue_key: String,
    issue_summary: String,
    date: NaiveDate,
    entries: Vec<WorklogEntry>,
    hours_per_day: f64,
    hours_per_week: f64,
    #[prop(default = Vec::new())] suggested_comments: Vec<String>,
    suggested_comment: Option<String>,
    #[prop(default = Vec::new())] commit_messages: Vec<String>,
    #[prop(default = Vec::new())] commit_links: Vec<String>,
    #[prop(default = Vec::new())] test_result_links: Vec<String>,
    #[prop(default = Vec::new())] pr_links: Vec<String>,
    is_git_log: bool,
    #[prop(default = false)] is_weekend: bool,
    restored_timer_popup: Option<PersistedTimerPopup>,
    /// Whether this popup's date column is today (enables timer controls).
    #[prop(default = false)]
    is_today: bool,
    on_close: Callback<()>,
    on_changed: Callback<String>,
    #[prop(default = String::new())] site_url: String,
    #[prop(default = None)] initial_digit: Option<char>,
) -> impl IntoView {
    let i18n = use_context::<RwSignal<I18n>>().unwrap_or_else(|| {
        log::error!("I18n context not provided in CellPopup, using English fallback");
        RwSignal::new(I18n::default())
    });
    let conn = use_connection();
    let timer_mgr = use_timer();
    let flush_mgr = use_popup_flush();

    let issue_key_for_close = issue_key.clone();
    let date_for_close = date;

    #[cfg(feature = "hydrate")]
    if let Some(digit) = initial_digit {
        schedule_initial_digit_fill(popup_id, digit);
    }
    #[cfg(not(feature = "hydrate"))]
    let _ = initial_digit;

    let restored_popup = restored_timer_popup.filter(|popup| {
        popup.issue_key == issue_key && popup.date == date && !popup.rows.is_empty()
    });
    let restored_existing_rows: HashMap<String, PersistedTimerRow> = restored_popup
        .as_ref()
        .map(|popup| {
            popup
                .rows
                .iter()
                .filter_map(|row| row.worklog_id.clone().map(|id| (id, row.clone())))
                .collect()
        })
        .unwrap_or_default();
    let restored_rows_by_index: HashMap<usize, PersistedTimerRow> = restored_popup
        .as_ref()
        .map(|popup| {
            popup
                .rows
                .iter()
                .map(|row| (row.row_index, row.clone()))
                .collect()
        })
        .unwrap_or_default();
    let restored_new_rows: Vec<PersistedTimerRow> = restored_popup
        .as_ref()
        .map(|popup| {
            popup
                .rows
                .iter()
                .filter(|row| row.worklog_id.is_none() && row.row_index >= 1000)
                .cloned()
                .collect()
        })
        .unwrap_or_default();

    // Create editable signals for each existing entry
    let existing: Vec<ExistingEntry> = entries
        .iter()
        .map(|e| {
            let w = i18n.get_untracked();
            let hours_text = format_hours_long(
                e.hours,
                hours_per_day,
                hours_per_week,
                &w.t(keys::WEEK_ABBR),
                &w.t(keys::DAY_ABBR),
                &w.t(keys::HOUR_ABBR),
                &w.t(keys::MINUTE_ABBR),
            );
            let display_comment = e.comment.split_whitespace().collect::<Vec<_>>().join(" ");
            let restored_row = restored_existing_rows.get(&e.id);
            ExistingEntry {
                id: e.id.clone(),
                hours_sig: RwSignal::new(
                    restored_row
                        .map(|row| row.hours_text.clone())
                        .unwrap_or_else(|| hours_text.clone()),
                ),
                initial_hours: hours_text,
                comment_sig: RwSignal::new(
                    restored_row
                        .map(|row| row.comment_text.clone())
                        .unwrap_or_else(|| display_comment.clone()),
                ),
                display_comment,
                comment_html: e.comment_html.clone(),
                comment_adf: e.comment_adf.clone(),
                deleted: RwSignal::new(false),
            }
        })
        .collect();

    // ── Dynamic new entry rows ──────────────────────────────────────────
    // Each new row is a tuple of (hours_signal, comment_signal).
    // There is always at least one (the blank "extra" row).
    let initial_prefills: Vec<String> = suggested_comments;
    let prefills_for_links = initial_prefills.clone();
    let mut unique_links = Vec::new();
    let mut seen_links = HashSet::new();
    for link in commit_links {
        if seen_links.insert(link.clone()) {
            unique_links.push(link);
        }
    }
    let commit_link_refs: Vec<(String, String)> = unique_links
        .into_iter()
        .enumerate()
        .map(|(idx, link)| {
            let message = commit_messages
                .get(idx)
                .cloned()
                .unwrap_or_else(String::new);
            (link, message)
        })
        .collect();
    let mut commit_refs_for_rows = commit_link_refs.clone();
    let mut pr_links_unique = pr_links;
    pr_links_unique.sort();
    pr_links_unique.dedup();
    let popup_pr_link = pr_links_unique.into_iter().next();
    let mut test_links_unique = test_result_links;
    test_links_unique.sort();
    test_links_unique.dedup();
    let popup_test_link = test_links_unique.into_iter().next();
    let suggested_row_links: Vec<Option<(String, String)>> = prefills_for_links
        .iter()
        .map(|comment| {
            if comment.trim().eq_ignore_ascii_case("review") {
                None
            } else if commit_refs_for_rows.is_empty() {
                None
            } else {
                Some(commit_refs_for_rows.remove(0))
            }
        })
        .collect();
    let mut initial_rows: Vec<(RwSignal<String>, RwSignal<String>)> = initial_prefills
        .into_iter()
        .map(|comment| (RwSignal::new(String::new()), RwSignal::new(comment)))
        .collect();
    initial_rows.push((RwSignal::new(String::new()), RwSignal::new(String::new())));
    let initial_new_entries: Vec<(RwSignal<String>, RwSignal<String>)> =
        if restored_new_rows.is_empty() {
            initial_rows
        } else {
            let mut restored = restored_new_rows
                .iter()
                .map(|row| {
                    (
                        RwSignal::new(row.hours_text.clone()),
                        RwSignal::new(row.comment_text.clone()),
                    )
                })
                .collect::<Vec<_>>();
            restored.push((
                RwSignal::new(String::new()),
                RwSignal::new(if is_git_log {
                    suggested_comment.clone().unwrap_or_default()
                } else {
                    String::new()
                }),
            ));
            restored
        };
    let new_entries: RwSignal<Vec<(RwSignal<String>, RwSignal<String>)>> =
        RwSignal::new(initial_new_entries);

    // ── Drag support ──
    // We use Rc<Cell<>> for drag state because these closures
    // are registered on the window and outlive the reactive
    // scope. RwSignals would panic when accessed after
    // disposal.
    //
    // An Arc<AtomicBool> "alive" flag is shared between the
    // window-level listeners and the close callback. When the
    // popup closes the flag is set to false and the forgotten
    // closures become harmless no-ops.
    let alive = Arc::new(AtomicBool::new(true));
    let drag_start_x = Rc::new(Cell::new(0.0f64));
    let drag_start_y = Rc::new(Cell::new(0.0f64));
    let drag_start_left = Rc::new(Cell::new(0.0f64));
    let drag_start_top = Rc::new(Cell::new(0.0f64));
    let is_dragging = Rc::new(Cell::new(false));

    // When the popup is closed, mark the alive flag as false
    // so the forgotten window listeners become no-ops, and
    // unregister from the flush manager.
    let alive_for_close = alive.clone();
    let orig_on_close = on_close;
    let flush_mgr_for_close = flush_mgr.clone();
    let on_close = Callback::new(move |v: ()| {
        alive_for_close.store(false, std::sync::atomic::Ordering::Relaxed);
        flush_mgr_for_close.unregister(popup_id);
        orig_on_close.run(v);
    });

    let issue_key_clone = issue_key.clone();
    let ik = issue_key.clone();
    let restored_new_rows_for_restore = restored_new_rows.clone();
    let issue_summary_for_persist = issue_summary.clone();
    let suggested_comment_for_persist = suggested_comment.clone();
    let saving_timer_rows = Rc::new(Cell::new(false));
    let persist_tick = RwSignal::new(0u64);

    let persist_timer_popup: Rc<dyn Fn()> = {
        let existing = existing.clone();
        let timer_mgr = timer_mgr.clone();
        let new_entries = new_entries;
        let pos_sig = pos_sig;
        let issue_key = issue_key.clone();
        let issue_summary = issue_summary_for_persist.clone();
        let suggested_comment = suggested_comment_for_persist.clone();
        Rc::new(move || {
            let mut rows = Vec::new();

            for (row_idx, entry) in existing.iter().enumerate() {
                if entry.deleted.get_untracked() {
                    continue;
                }

                let timer_id = TimerId {
                    issue_key: issue_key.clone(),
                    date,
                    row_index: row_idx,
                };
                let Some(timer_state) = timer_mgr.persisted_state(&timer_id) else {
                    continue;
                };

                rows.push(PersistedTimerRow {
                    row_index: row_idx,
                    worklog_id: Some(entry.id.clone()),
                    hours_text: entry.hours_sig.get_untracked(),
                    comment_text: if entry.comment_html.is_empty() {
                        entry.comment_sig.get_untracked()
                    } else {
                        entry.display_comment.clone()
                    },
                    timer_state,
                });
            }

            let existing_count = existing.len();
            let new_rows = new_entries.get_untracked();
            for (idx, (hours_sig, comment_sig)) in new_rows.iter().enumerate() {
                let timer_id = TimerId {
                    issue_key: issue_key.clone(),
                    date,
                    row_index: existing_count + 1000 + idx,
                };
                let Some(timer_state) = timer_mgr.persisted_state(&timer_id) else {
                    continue;
                };

                rows.push(PersistedTimerRow {
                    row_index: existing_count + 1000 + idx,
                    worklog_id: None,
                    hours_text: hours_sig.get_untracked(),
                    comment_text: comment_sig.get_untracked(),
                    timer_state,
                });
            }

            if rows.is_empty() {
                return;
            }

            save_persisted_timer_popup(PersistedTimerPopup {
                issue_key: issue_key.clone(),
                issue_summary: issue_summary.clone(),
                date,
                suggested_comment: suggested_comment.clone(),
                is_git_log,
                is_weekend,
                position_style: Some(pos_sig.get_untracked()),
                rows,
            });
        })
    };

    let popup_has_active_timers: Rc<dyn Fn() -> bool> = {
        let existing = existing.clone();
        let timer_mgr = timer_mgr.clone();
        let new_entries = new_entries;
        let issue_key = issue_key.clone();
        Rc::new(move || {
            for (row_idx, entry) in existing.iter().enumerate() {
                if entry.deleted.get_untracked() {
                    continue;
                }
                let timer_id = TimerId {
                    issue_key: issue_key.clone(),
                    date,
                    row_index: row_idx,
                };
                if timer_mgr.is_active_untracked(&timer_id) {
                    return true;
                }
            }

            let existing_count = existing.len();
            let new_rows = new_entries.get_untracked();
            for idx in 0..new_rows.len() {
                let timer_id = TimerId {
                    issue_key: issue_key.clone(),
                    date,
                    row_index: existing_count + 1000 + idx,
                };
                if timer_mgr.is_active_untracked(&timer_id) {
                    return true;
                }
            }

            false
        })
    };

    for (row_idx, entry) in existing.iter().enumerate() {
        if let Some(restored_row) = restored_existing_rows
            .get(&entry.id)
            .or_else(|| restored_rows_by_index.get(&row_idx))
        {
            timer_mgr.restore_persisted_state(
                TimerId {
                    issue_key: issue_key.clone(),
                    date,
                    row_index: row_idx,
                },
                entry.hours_sig,
                hours_per_day,
                hours_per_week,
                i18n.get_untracked().decimal_separator,
                restored_row.timer_state.clone(),
            );
        }
    }

    let restored_new_entries = new_entries.get_untracked();
    let existing_count = existing.len();
    for (idx, restored_row) in restored_new_rows_for_restore.iter().enumerate() {
        if let Some((hours_sig, _)) = restored_new_entries.get(idx) {
            timer_mgr.restore_persisted_state(
                TimerId {
                    issue_key: issue_key.clone(),
                    date,
                    row_index: existing_count + 1000 + idx,
                },
                *hours_sig,
                hours_per_day,
                hours_per_week,
                i18n.get_untracked().decimal_separator,
                restored_row.timer_state.clone(),
            );
        }
    }

    Effect::new({
        let existing = existing.clone();
        let timer_mgr = timer_mgr.clone();
        let new_entries = new_entries;
        let issue_key = issue_key.clone();
        let persist_timer_popup = persist_timer_popup.clone();
        let persist_tick = persist_tick;
        move |_| {
            let _ = persist_tick.get();
            for (row_idx, entry) in existing.iter().enumerate() {
                let _ = entry.deleted.get();
                let _ = entry.hours_sig.get();
                let _ = entry.comment_sig.get();
                let timer_id = TimerId {
                    issue_key: issue_key.clone(),
                    date,
                    row_index: row_idx,
                };
                let _ = timer_mgr.phase(&timer_id);
            }

            let existing_count = existing.len();
            let rows = new_entries.get();
            for (idx, (hours_sig, comment_sig)) in rows.iter().enumerate() {
                let _ = hours_sig.get();
                let _ = comment_sig.get();
                let timer_id = TimerId {
                    issue_key: issue_key.clone(),
                    date,
                    row_index: existing_count + 1000 + idx,
                };
                let _ = timer_mgr.phase(&timer_id);
            }

            persist_timer_popup();
        }
    });

    #[cfg(feature = "hydrate")]
    {
        use gloo_timers::callback::Interval;

        let alive = alive.clone();
        let persist_timer_popup = persist_timer_popup.clone();
        Interval::new(60_000, move || {
            if !alive.load(std::sync::atomic::Ordering::Relaxed) {
                return;
            }
            persist_timer_popup();
        })
        .forget();
    }

    // ── Validation (Save enablement) ────────────────────────────────────
    let existing_for_validation = existing.clone();
    let popup_validations = Memo::new(move |_| {
        let w = i18n.get();
        let dec_sep = w.decimal_separator;
        let wl = w.t(keys::WEEK_ABBR);
        let dl = w.t(keys::DAY_ABBR);
        let hl = w.t(keys::HOUR_ABBR);
        let ml = w.t(keys::MINUTE_ABBR);
        let duration_positive_error = i18n.get().t(keys::POPUP_ERROR_DURATION_POSITIVE);
        let description_required_error = i18n.get().t(keys::POPUP_ERROR_DESCRIPTION_REQUIRED);
        let description_unique_error = i18n.get().t(keys::POPUP_ERROR_DESCRIPTION_UNIQUE);

        let mut existing_validations =
            vec![PopupRowValidation::default(); existing_for_validation.len()];
        let rows = new_entries.get();
        let mut new_validations = vec![PopupRowValidation::default(); rows.len()];
        let mut duration_rows: Vec<(bool, usize, String)> = Vec::new();
        let mut has_non_blank_row = false;
        let has_deletes = existing_for_validation.iter().any(|e| e.deleted.get());

        for (idx, entry) in existing_for_validation.iter().enumerate() {
            if entry.deleted.get() {
                continue;
            }
            let hours = entry.hours_sig.get();
            let description = if !entry.comment_html.is_empty() {
                entry.display_comment.clone()
            } else {
                entry.comment_sig.get()
            };
            let blank_row = hours.trim().is_empty() && description.trim().is_empty();
            if blank_row {
                continue;
            }
            has_non_blank_row = true;
            let parsed = parse_hours(
                &hours,
                hours_per_day,
                hours_per_week,
                dec_sep,
                &wl,
                &dl,
                &hl,
                &ml,
            );
            if let Some(parsed_hours) = parsed {
                if parsed_hours > 0.0 {
                    duration_rows.push((true, idx, description));
                } else {
                    existing_validations[idx].hours_error = Some(duration_positive_error.clone());
                }
            } else {
                existing_validations[idx].hours_error = Some(duration_positive_error.clone());
            }
        }

        for (idx, (hours_sig, comment_sig)) in rows.iter().enumerate() {
            let hours = hours_sig.get();
            let description = comment_sig.get();
            let blank_row = hours.trim().is_empty() && description.trim().is_empty();
            if blank_row {
                continue;
            }
            has_non_blank_row = true;
            let parsed = parse_hours(
                &hours,
                hours_per_day,
                hours_per_week,
                dec_sep,
                &wl,
                &dl,
                &hl,
                &ml,
            );
            if let Some(parsed_hours) = parsed {
                if parsed_hours > 0.0 {
                    duration_rows.push((false, idx, description));
                } else {
                    new_validations[idx].hours_error = Some(duration_positive_error.clone());
                }
            } else {
                new_validations[idx].hours_error = Some(duration_positive_error.clone());
            }
        }

        if duration_rows.len() > 1 {
            let mut seen: HashMap<String, (bool, usize)> = HashMap::new();
            for (is_existing, row_idx, description) in &duration_rows {
                let trimmed = description.trim();
                if trimmed.is_empty() {
                    if *is_existing {
                        existing_validations[*row_idx].description_error =
                            Some(description_required_error.clone());
                    } else {
                        new_validations[*row_idx].description_error =
                            Some(description_required_error.clone());
                    }
                    continue;
                }
                let normalized = normalize_popup_description(trimmed);
                if let Some((prior_existing, prior_idx)) = seen.get(&normalized) {
                    if *is_existing {
                        existing_validations[*row_idx].description_error =
                            Some(description_unique_error.clone());
                    } else {
                        new_validations[*row_idx].description_error =
                            Some(description_unique_error.clone());
                    }
                    if *prior_existing {
                        existing_validations[*prior_idx].description_error =
                            Some(description_unique_error.clone());
                    } else {
                        new_validations[*prior_idx].description_error =
                            Some(description_unique_error.clone());
                    }
                } else {
                    seen.insert(normalized, (*is_existing, *row_idx));
                }
            }
        }

        let has_errors = existing_validations.iter().any(|v| !v.is_valid())
            || new_validations.iter().any(|v| !v.is_valid());
        let can_save = if has_errors {
            false
        } else if has_non_blank_row {
            true
        } else {
            has_deletes
        };

        (can_save, existing_validations, new_validations)
    });

    let popup_validations_for_save = popup_validations.clone();
    let save_enabled = Memo::new(move |_| popup_validations_for_save.get().0);
    let popup_validations_for_existing = popup_validations.clone();
    let existing_row_validations = Memo::new(move |_| popup_validations_for_existing.get().1);
    let new_row_validations = Memo::new(move |_| popup_validations.get().2);

    // ── Save handler ────────────────────────────────────────────────────
    let existing_for_save = existing.clone();
    let on_save = {
        let ik = issue_key.clone();
        let issue_key_for_stop = issue_key.clone();
        let saving_timer_rows = saving_timer_rows.clone();
        move |latch: Option<FlushLatch>| {
            let w = i18n.get_untracked();
            let dec_sep = w.decimal_separator;
            let wl = w.t(keys::WEEK_ABBR);
            let dl = w.t(keys::DAY_ABBR);
            let hl = w.t(keys::HOUR_ABBR);
            let ml = w.t(keys::MINUTE_ABBR);
            let ik = ik.clone();

            // Stop all timers for this popup on save
            saving_timer_rows.set(true);
            timer_mgr.stop_all_for_popup(&issue_key_for_stop, date);

            let deletes: Vec<(String, String)> = existing_for_save
                .iter()
                .filter(|e| e.deleted.get_untracked())
                .map(|e| (ik.clone(), e.id.clone()))
                .collect();

            let mut updates: Vec<(String, String, f64, String, Option<String>)> = Vec::new();
            for entry in existing_for_save.iter() {
                if entry.deleted.get_untracked() {
                    continue;
                }
                let hours_text = entry.hours_sig.get_untracked();

                let (comment_text, adf) = if !entry.comment_html.is_empty() {
                    (entry.display_comment.clone(), entry.comment_adf.clone())
                } else {
                    let current = entry.comment_sig.get_untracked();
                    let adf = if current == entry.display_comment {
                        entry.comment_adf.clone()
                    } else {
                        None
                    };
                    (current, adf)
                };

                if let Some(h) = parse_hours(
                    &hours_text,
                    hours_per_day,
                    hours_per_week,
                    dec_sep,
                    &wl,
                    &dl,
                    &hl,
                    &ml,
                ) {
                    updates.push((ik.clone(), entry.id.clone(), h, comment_text, adf));
                }
            }

            let rows = new_entries.get_untracked();
            let mut creates: Vec<(String, NaiveDate, f64, String)> = Vec::new();
            for (idx, (h_sig, c_sig)) in rows.iter().enumerate() {
                let h_text = h_sig.get_untracked();
                let c_text = c_sig.get_untracked();
                let is_last = idx == rows.len() - 1;
                if is_last && h_text.is_empty() && c_text.is_empty() {
                    continue;
                }
                if let Some(h) = parse_hours(
                    &h_text,
                    hours_per_day,
                    hours_per_week,
                    dec_sep,
                    &wl,
                    &dl,
                    &hl,
                    &ml,
                ) {
                    creates.push((ik.clone(), date, h, c_text));
                }
            }

            on_close.run(());

            leptos::task::spawn_local(async move {
                let issue_key_for_changed = ik.clone();
                conn.request_started();
                let mut errors: Vec<String> = Vec::new();
                for (ik, id) in deletes {
                    if let Err(e) = server_delete_worklog(ik, id, new_request_nonce()).await {
                        errors.push(format!("Delete failed: {}", e));
                    }
                }
                for (ik, id, h, comment, adf) in updates {
                    if let Err(e) =
                        server_update_worklog(ik, id, h, comment, adf, new_request_nonce()).await
                    {
                        errors.push(format!("Update failed: {}", e));
                    }
                }
                for (ik, date, h, comment) in creates {
                    if let Err(e) =
                        server_add_worklog(ik, date, h, comment, new_request_nonce()).await
                    {
                        errors.push(format!("Save failed: {}", e));
                    }
                }
                conn.request_finished();
                if !errors.is_empty() {
                    let msg = errors.join("\n");
                    log::error!("[CellPopup] Worklog save error(s): {}", msg);
                    #[cfg(feature = "hydrate")]
                    web_sys::window().map(|w| w.alert_with_message(&msg));
                }
                // Signal the flush latch (if any) *before* refetch so that
                // callers waiting on the latch (e.g. language-switch reload)
                // can proceed now that the server state is up-to-date.
                if let Some(latch) = latch {
                    latch.arrive();
                }
                if errors.is_empty() {
                    remove_persisted_timer_popup(&ik, date);
                }
                on_changed.run(issue_key_for_changed);
            });
        }
    };

    // ── Flush-on-navigate registration ──────────────────────────────────
    // Build is_dirty / is_valid closures that inspect popup signals without
    // subscribing (get_untracked), then register with PopupDraftManager so
    // navigation actions can auto-save this popup.
    {
        let existing_for_dirty = existing.clone();
        let popup_has_active_timers = popup_has_active_timers.clone();
        let is_dirty: Rc<dyn Fn() -> bool> = Rc::new(move || {
            if popup_has_active_timers() {
                return false;
            }
            // Any existing entry deleted?
            if existing_for_dirty.iter().any(|e| e.deleted.get_untracked()) {
                return true;
            }
            // Any existing entry hours or comment changed?
            for entry in existing_for_dirty.iter() {
                if entry.hours_sig.get_untracked() != entry.initial_hours {
                    return true;
                }
                if entry.comment_html.is_empty()
                    && entry.comment_sig.get_untracked() != entry.display_comment
                {
                    return true;
                }
            }
            // Any new entry row with non-empty hours?
            let rows = new_entries.get_untracked();
            for (idx, (h_sig, _)) in rows.iter().enumerate() {
                let h = h_sig.get_untracked();
                let is_last = idx == rows.len() - 1;
                if is_last && h.is_empty() {
                    continue;
                }
                if !h.is_empty() {
                    return true;
                }
            }
            false
        });

        let is_valid: Rc<dyn Fn() -> bool> = {
            let save_enabled = save_enabled.clone();
            Rc::new(move || save_enabled.get_untracked() && conn.is_available())
        };

        let on_save_for_flush = on_save.clone();
        let save_fn: Rc<dyn Fn(Option<FlushLatch>)> = Rc::new(move |latch: Option<FlushLatch>| {
            on_save_for_flush(latch);
        });

        let is_dirty_reg = is_dirty.clone();
        let is_valid_reg = is_valid.clone();
        let save_fn_reg = save_fn.clone();
        flush_mgr.register(
            popup_id,
            move || (is_valid_reg)(),
            move || (is_dirty_reg)(),
            move |latch| (save_fn_reg)(latch),
        );
    }

    // ── Close handler that also stops timers ────────────────────────────
    let on_close_with_timers = {
        let ik = issue_key_for_close.clone();
        let saving_timer_rows = saving_timer_rows.clone();
        move || {
            saving_timer_rows.set(false);
            timer_mgr.stop_all_for_popup(&ik, date_for_close);
            remove_persisted_timer_popup(&ik, date_for_close);
            on_close.run(());
        }
    };

    // ── Keyboard shortcuts ──────────────────────────────────────────────
    let on_keydown = {
        let on_save = on_save.clone();
        let on_close_with_timers = on_close_with_timers.clone();
        move |ev: leptos::ev::KeyboardEvent| {
            let key = ev.key();
            // Ctrl+Arrow: move popup. Ctrl+Alt+Arrow: 1px; Ctrl+Arrow: 1 char/line.
            if ev.ctrl_key() {
                let delta: f64 = match key.as_str() {
                    "ArrowLeft" | "ArrowRight" if ev.alt_key() => 1.0,
                    "ArrowUp" | "ArrowDown" if ev.alt_key() => 1.0,
                    "ArrowLeft" | "ArrowRight" => 8.0,
                    "ArrowUp" | "ArrowDown" => 20.0,
                    _ => 0.0,
                };
                if delta > 0.0 {
                    ev.prevent_default();
                    ev.stop_propagation();
                    let cur = pos_sig.get_untracked();
                    let (mut left, mut top) = parse_popup_pos(&cur);
                    match key.as_str() {
                        "ArrowLeft" => left -= delta,
                        "ArrowRight" => left += delta,
                        "ArrowUp" => top -= delta,
                        "ArrowDown" => top += delta,
                        _ => {}
                    }
                    pos_sig.set(format!("left:{:.0}px;top:{:.0}px", left, top));
                    return;
                }
            }
            match key.as_str() {
                "Enter" if ev.ctrl_key() => {
                    if save_enabled.get() && conn.is_available() {
                        ev.prevent_default();
                        ev.stop_propagation();
                        on_save(None);
                    }
                }
                "Escape" => {
                    ev.prevent_default();
                    ev.stop_propagation();
                    on_close_with_timers();
                }
                _ => {}
            } // match key
        } // closure
    };

    // ── Blur handler for dynamic new rows ───────────────────────────────
    let on_new_hours_blur = move |idx: usize| {
        let w = i18n.get_untracked();
        let dec_sep = w.decimal_separator;
        let wl = w.t(keys::WEEK_ABBR);
        let dl = w.t(keys::DAY_ABBR);
        let hl = w.t(keys::HOUR_ABBR);
        let ml = w.t(keys::MINUTE_ABBR);
        let rows = new_entries.get_untracked();
        if idx != rows.len() - 1 {
            return;
        }
        if let Some((hours_sig, _)) = rows.last() {
            let hours = hours_sig.get_untracked();
            if !hours.is_empty()
                && parse_hours(
                    &hours,
                    hours_per_day,
                    hours_per_week,
                    dec_sep,
                    &wl,
                    &dl,
                    &hl,
                    &ml,
                )
                .is_some()
            {
                leptos::task::spawn_local(async move {
                    new_entries.update(|vec| {
                        vec.push((RwSignal::new(String::new()), RwSignal::new(String::new())));
                    });
                });
            }
        }
    };

    // ── Helper: build timer buttons for a row ───────────────────────────
    // Returns a view fragment with play/pause + stop buttons, or an empty
    // spacer when timers are not applicable.
    let build_timer_buttons = move |timer_id: TimerId, hours_sig: RwSignal<String>| {
        if !is_today && !timer_mgr.is_active_untracked(&timer_id) {
            return view! { <span class="popup-spacer"></span> }.into_any();
        }

        let tid_for_phase = timer_id.clone();
        let tid_start = timer_id.clone();
        let tid_pause = timer_id.clone();
        let tid_resume = timer_id.clone();
        let tid_persist = timer_id.clone();
        let persist_tick = persist_tick;
        let issue_summary = issue_summary_for_persist.clone();
        let suggested_comment = suggested_comment_for_persist.clone();
        let pos_sig = pos_sig;
        let on_play_pause = move |_| {
            let phase = timer_mgr.phase_untracked(&tid_for_phase);
            let dec_sep = i18n.get_untracked().decimal_separator;
            match phase {
                None | Some(TimerPhase::Stopped) => {
                    timer_mgr.start(
                        tid_start.clone(),
                        hours_sig,
                        hours_per_day,
                        hours_per_week,
                        dec_sep,
                    );
                }
                Some(TimerPhase::Running) => {
                    timer_mgr.pause(&tid_pause);
                }
                Some(TimerPhase::Paused { .. }) => {
                    let dec_sep = i18n.get_untracked().decimal_separator;
                    timer_mgr.resume(&tid_resume, dec_sep);
                }
            }
            ensure_timer_storage_initialized();
            if let Some(timer_state) = timer_mgr.persisted_state(&tid_persist) {
                upsert_persisted_timer_row(
                    &tid_persist.issue_key,
                    &issue_summary,
                    tid_persist.date,
                    suggested_comment.clone(),
                    is_git_log,
                    is_weekend,
                    Some(pos_sig.get_untracked()),
                    PersistedTimerRow {
                        row_index: tid_persist.row_index,
                        worklog_id: None,
                        hours_text: hours_sig.get_untracked(),
                        comment_text: String::new(),
                        timer_state,
                    },
                );
            } else if let Some(phase_now) = timer_mgr.phase_untracked(&tid_persist) {
                let fallback_state = match phase_now {
                    TimerPhase::Running => PersistedTimerState {
                        phase: PersistedTimerPhase::Running,
                        is_first_interval: true,
                        remaining_ms: 1,
                        elapsed_ms: 0,
                        generation: 0,
                        snapshot_at_epoch_ms: None,
                    },
                    TimerPhase::Paused { remaining_ms } => PersistedTimerState {
                        phase: PersistedTimerPhase::Paused,
                        is_first_interval: true,
                        remaining_ms,
                        elapsed_ms: 0,
                        generation: 0,
                        snapshot_at_epoch_ms: None,
                    },
                    TimerPhase::Stopped => {
                        persist_tick.update(|value| *value += 1);
                        return;
                    }
                };
                upsert_persisted_timer_row(
                    &tid_persist.issue_key,
                    &issue_summary,
                    tid_persist.date,
                    suggested_comment.clone(),
                    is_git_log,
                    is_weekend,
                    Some(pos_sig.get_untracked()),
                    PersistedTimerRow {
                        row_index: tid_persist.row_index,
                        worklog_id: None,
                        hours_text: hours_sig.get_untracked(),
                        comment_text: String::new(),
                        timer_state: fallback_state,
                    },
                );
            }
            persist_tick.update(|value| *value += 1);
        };

        let tid_phase_display = timer_id.clone();
        let tid_phase_display2 = timer_id.clone();

        view! {
            <span class="timer-controls">
                <button
                    class="timer-btn timer-play-pause"
                    on:click=on_play_pause
                    title={move || {
                        let phase = timer_mgr.phase(&tid_phase_display);
                        match phase {
                            None | Some(TimerPhase::Stopped) => i18n.get().t(keys::TIMER_START),
                            Some(TimerPhase::Running) => i18n.get().t(keys::TIMER_PAUSE),
                            Some(TimerPhase::Paused { .. }) => i18n.get().t(keys::TIMER_RESUME),
                        }
                    }}
                >
                    {move || {
                        let phase = timer_mgr.phase(&tid_phase_display2);
                        match phase {
                            Some(TimerPhase::Running) => "\u{23F8}",  // ⏸ pause
                            Some(TimerPhase::Paused { .. }) => "\u{25B6}",  // ▶ play (resume)
                            _ => "\u{25B6}",  // ▶ play (start)
                        }
                    }}
                </button>
            </span>
        }
        .into_any()
    };

    let on_close_with_timers_for_btn = on_close_with_timers.clone();
    let on_title_action_mousedown = move |ev: leptos::ev::MouseEvent| {
        ev.stop_propagation();
    };

    // ── Drag support ──
    #[cfg(feature = "hydrate")]
    {
        use wasm_bindgen::prelude::*;

        let dragging = is_dragging.clone();
        let sx = drag_start_x.clone();
        let sy = drag_start_y.clone();
        let sl = drag_start_left.clone();
        let st = drag_start_top.clone();
        let alive_move = alive.clone();

        // mousemove handler (registered on window)
        let pos_sig_move = pos_sig;
        let move_cb =
            Closure::<dyn Fn(web_sys::MouseEvent)>::new(move |ev: web_sys::MouseEvent| {
                if !alive_move.load(std::sync::atomic::Ordering::Relaxed) {
                    return;
                }
                if !dragging.get() {
                    return;
                }
                let dx = ev.client_x() as f64 - sx.get();
                let dy = ev.client_y() as f64 - sy.get();
                let new_left = sl.get() + dx;
                let new_top = st.get() + dy;
                pos_sig_move.set(format!("left:{new_left:.0}px;top:{new_top:.0}px"));
            });

        let dragging_up = is_dragging.clone();
        let alive_up = alive.clone();
        let up_cb = Closure::<dyn Fn(web_sys::MouseEvent)>::new(move |_: web_sys::MouseEvent| {
            if !alive_up.load(std::sync::atomic::Ordering::Relaxed) {
                return;
            }
            dragging_up.set(false);
        });

        if let Some(window) = web_sys::window() {
            let _ = window
                .add_event_listener_with_callback("mousemove", move_cb.as_ref().unchecked_ref());
            let _ =
                window.add_event_listener_with_callback("mouseup", up_cb.as_ref().unchecked_ref());
        }
        move_cb.forget();
        up_cb.forget();
    }

    let popup_ref: NodeRef<leptos::html::Div> = NodeRef::new();

    // Focus the first input after the popup mounts. We defer via
    // requestAnimationFrame so that child components (inputs) are in the DOM.
    // Restored timer popups open automatically on page load; skip stealing
    // focus from the grid in that case.
    #[cfg(feature = "hydrate")]
    if restored_popup.is_none() {
        use leptos::wasm_bindgen::JsCast;
        use leptos::wasm_bindgen::closure::Closure;
        let popup_ref_focus = popup_ref;
        Effect::new(move |_| {
            if let Some(el) = popup_ref_focus.get() {
                let root_html: web_sys::HtmlElement =
                    el.unchecked_ref::<web_sys::HtmlElement>().clone();
                let cb = Closure::once(move || {
                    if let Ok(Some(node)) =
                        root_html.query_selector("input:not([disabled]),textarea:not([disabled])")
                    {
                        if let Ok(input) = node.dyn_into::<web_sys::HtmlElement>() {
                            let _ = input.focus();
                        }
                    }
                });
                if let Some(window) = web_sys::window() {
                    let _ = window.request_animation_frame(cb.as_ref().unchecked_ref());
                }
                cb.forget();
            }
        });
    }

    let dragging_md = is_dragging.clone();
    let sx_md = drag_start_x.clone();
    let sy_md = drag_start_y.clone();
    let sl_md = drag_start_left.clone();
    let st_md = drag_start_top.clone();
    let on_header_mousedown = move |ev: leptos::ev::MouseEvent| {
        dragging_md.set(true);
        sx_md.set(ev.client_x() as f64);
        sy_md.set(ev.client_y() as f64);
        // Read current left/top directly from the DOM element to avoid
        // borrowing pos_sig's RefCell (which may already be borrowed by
        // the reactive style closure during resize, causing a panic).
        let (left, top) = popup_ref.get().map_or((0.0, 0.0), |el| {
            use leptos::wasm_bindgen::JsCast;
            let html: &leptos::web_sys::HtmlElement = el.unchecked_ref();
            (html.offset_left() as f64, html.offset_top() as f64)
        });
        sl_md.set(left);
        st_md.set(top);
        ev.prevent_default();
    };

    let z_index =
        RwSignal::new(POPUP_Z_COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed) + 1);

    let on_focusin = move |_: leptos::ev::FocusEvent| {
        let next = POPUP_Z_COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed) + 1;
        z_index.set(next);
    };

    //
    view! {
         <div
            class="cell-popup"
            node_ref=popup_ref
            data-popup-id={popup_id.to_string()}
            style=move || format!("{};z-index:{}", pos_sig.get(), z_index.get())
            on:keydown=on_keydown
            on:focusin=on_focusin
         >
            <div class="popup-draggable-title" on:mousedown=on_header_mousedown>
                <span class="popup-key">{issue_key}</span>
                <span class="popup-summary" title={issue_summary}>{issue_summary.clone()}</span>
                <span class="popup-date">{i18n.get_untracked().format_date(&date)}</span>
                <span class="popup-title-actions" on:mousedown=on_title_action_mousedown>
                    <button
                        class="popup-title-action popup-title-save"
                        tabindex="-1"
                        on:click=move |_| on_save(None)
                        disabled=move || !save_enabled.get() || !conn.is_available()
                        title=move || i18n.get().t(keys::SAVE)
                    >
                        {"✓"}
                    </button>
                    <button
                        class="popup-title-action popup-title-close"
                        tabindex="-1"
                        on:click=move |_| on_close_with_timers_for_btn()
                        title=move || i18n.get().t(keys::CLOSE)
                    >
                        {"×"}
                    </button>
                </span>
            </div>
        <div class="cell-popup-content" tabindex="0">
            <div class="popup-entries">
                // ── Existing entry rows ─────────────────────────────────
                {existing
                    .iter()
                    .enumerate()
                    .map(|(row_idx, entry)| {
                        let id = entry.id.clone();
                        let ik = ik.clone();
                        let has_html = !entry.comment_html.is_empty();
                        let comment_html = entry.comment_html.clone();
                        let hours_sig = entry.hours_sig;
                        let comment_sig = entry.comment_sig;
                        let deleted = entry.deleted;
                        let link_href = worklog_url(&site_url, &ik, &id);

                        let delete_worklog = move |_| {
                            deleted.set(true);
                        };

                        let timer_id = TimerId {
                            issue_key: ik.clone(),
                            date,
                            row_index: row_idx,
                        };
                        let tid_for_disabled = timer_id.clone();
                        let tid_for_progress = timer_id.clone();
                        let timer_buttons = build_timer_buttons(timer_id, hours_sig);

                        view! {
                            <div
                                class="popup-entry-group"
                                style:display=move || if deleted.get() { "none" } else { "" }
                            >
                                <div class="popup-entry">
                                    <div class="popup-hours-container">
                                        <input
                                            type="text"
                                            class="popup-hours"
                                            class:popup-field-invalid=move || {
                                                existing_row_validations
                                                    .get()
                                                    .get(row_idx)
                                                    .and_then(|row| row.hours_error.as_ref())
                                                    .is_some()
                                            }
                                            prop:value={move || hours_sig.get()}
                                            on:input=move |ev| hours_sig.set(event_target_value(&ev))
                                            placeholder={move || i18n.get().t(keys::HOURS)}
                                            disabled=move || !conn.is_available() || timer_mgr.is_active(&tid_for_disabled)
                                        />
                                        {move || {
                                            existing_row_validations
                                                .get()
                                                .get(row_idx)
                                                .and_then(|row| row.hours_error.clone())
                                                .map(|message| {
                                                    view! { <span class="popup-error-indicator" title={message}>!</span> }
                                                })
                                        }}
                                        {move || {
                                            let tid = tid_for_progress.clone();
                                            if let Some(ProgressInfo { is_first_interval, offset_ms, running, generation }) = timer_mgr.progress_info(&tid) {
                                                let anim = if generation % 2 == 0 { "even" } else { "odd" };
                                                let state = if running { "running" } else { "paused" };
                                                let cls = format!("timer-progress-bar {} {}", anim, state);
                                                let duration_var = if is_first_interval { "var(--timer-interval-short)" } else { "var(--timer-interval)" };
                                                view! {
                                                    <div
                                                        class=cls
                                                        style=format!("--timer-duration: {}; --timer-offset: -{}ms", duration_var, offset_ms)
                                                    ></div>
                                                }.into_any()
                                            } else {
                                                view! { <span class="timer-progress-spacer"></span> }.into_any()
                                            }
                                        }}
                                    </div>
                                    {if has_html {
                                        view! {
                                            <div class="popup-comment-container">
                                                <div
                                                    class="adf-preview"
                                                    class:popup-field-invalid=move || {
                                                        existing_row_validations
                                                            .get()
                                                            .get(row_idx)
                                                            .and_then(|row| row.description_error.as_ref())
                                                            .is_some()
                                                    }
                                                    inner_html={comment_html.clone()}
                                                ></div>
                                                {move || {
                                                    existing_row_validations
                                                        .get()
                                                        .get(row_idx)
                                                        .and_then(|row| row.description_error.clone())
                                                        .map(|message| {
                                                            view! { <span class="popup-error-indicator" title={message}>!</span> }
                                                        })
                                                }}
                                            </div>
                                        }.into_any()
                                    } else {
                                        view! {
                                            <div class="popup-comment-container">
                                                <textarea
                                                    class="popup-comment"
                                                    class:popup-field-invalid=move || {
                                                        existing_row_validations
                                                            .get()
                                                            .get(row_idx)
                                                            .and_then(|row| row.description_error.as_ref())
                                                            .is_some()
                                                    }
                                                    prop:value={move || comment_sig.get()}
                                                    on:input=move |ev| comment_sig.set(event_target_value(&ev))
                                                    placeholder={move || i18n.get().t(keys::DESCRIPTION)}
                                                    disabled=move || !conn.is_available()
                                                    rows="1"
                                                />
                                                {move || {
                                                    existing_row_validations
                                                        .get()
                                                        .get(row_idx)
                                                        .and_then(|row| row.description_error.clone())
                                                        .map(|message| {
                                                            view! { <span class="popup-error-indicator" title={message}>!</span> }
                                                        })
                                                }}
                                            </div>
                                        }.into_any()
                                    }}
                                    <span class="popup-actions">
                                        {timer_buttons}
                                        <a
                                            class="popup-link popup-link--jira"
                                            href={link_href.clone()}
                                            target="_blank"
                                            rel="noopener"
                                            title=move || i18n.get().t(keys::OPEN_IN_JIRA)
                                        >
                                            <span class="popup-link-base">"\u{1F517}"</span>
                                            <span class="popup-link-badge">"J"</span>
                                        </a>
                                        <button class="popup-delete" on:click=delete_worklog disabled=move || !conn.is_available()>
                                            "\u{1F5D1}"
                                        </button>
                                    </span>
                                </div>
                            </div>
                        }
                    })
                    .collect::<Vec<_>>()}

                // ── Dynamic new entry rows ──────────────────────────────
                {move || {
                    let rows = new_entries.get();
                    let existing_count = existing.len();
                    rows.into_iter().enumerate().map(|(idx, (hours_sig, comment_sig))| {
                        let on_blur = on_new_hours_blur.clone();
                        // New rows get timer IDs offset by existing count + 1000 to
                        // avoid collisions with existing entry indices.
                        let timer_id = TimerId {
                            issue_key: issue_key_clone.clone(),
                            date,
                            row_index: existing_count + 1000 + idx,
                        };
                        let tid_for_disabled = timer_id.clone();
                        let tid_for_progress = timer_id.clone();
                        let timer_buttons = build_timer_buttons(timer_id, hours_sig);
                        let row_link = suggested_row_links.get(idx).cloned().flatten();

                        view! {
                            <div class="popup-entry popup-new">
                                <div class="popup-hours-container">
                                    <input
                                        type="text"
                                        class="popup-hours"
                                        class:popup-field-invalid=move || {
                                            new_row_validations
                                                .get()
                                                .get(idx)
                                                .and_then(|row| row.hours_error.as_ref())
                                                .is_some()
                                        }
                                        prop:value={move || hours_sig.get()}
                                        on:input=move |ev| hours_sig.set(event_target_value(&ev))
                                        on:blur=move |_| on_blur(idx)
                                        placeholder={move || i18n.get().t(keys::HOURS)}
                                        disabled=move || !conn.is_available() || timer_mgr.is_active(&tid_for_disabled)
                                    />
                                    {move || {
                                        new_row_validations
                                            .get()
                                            .get(idx)
                                            .and_then(|row| row.hours_error.clone())
                                            .map(|message| {
                                                view! { <span class="popup-error-indicator" title={message}>!</span> }
                                            })
                                    }}
                                    {move || {
                                        let tid = tid_for_progress.clone();
                                        if let Some(ProgressInfo { is_first_interval, offset_ms, running, generation }) = timer_mgr.progress_info(&tid) {
                                            let anim = if generation % 2 == 0 { "even" } else { "odd" };
                                            let state = if running { "running" } else { "paused" };
                                            let cls = format!("timer-progress-bar {} {}", anim, state);
                                            let duration_var = if is_first_interval { "var(--timer-interval-short)" } else { "var(--timer-interval)" };
                                            view! {
                                                <div
                                                    class=cls
                                                    style=format!("--timer-duration: {}; --timer-offset: -{}ms", duration_var, offset_ms)
                                                ></div>
                                            }.into_any()
                                        } else {
                                            view! { <span class="timer-progress-spacer"></span> }.into_any()
                                        }
                                    }}
                                </div>
                                <div class="popup-comment-container">
                                    <textarea
                                        class="popup-comment popup-comment-new"
                                        class:popup-field-invalid=move || {
                                            new_row_validations
                                                .get()
                                                .get(idx)
                                                .and_then(|row| row.description_error.as_ref())
                                                .is_some()
                                        }
                                        prop:value={move || comment_sig.get()}
                                        on:input=move |ev| comment_sig.set(event_target_value(&ev))
                                        placeholder={move || i18n.get().t(keys::DESCRIPTION)}
                                        disabled=move || !conn.is_available()
                                        rows="1"
                                    />
                                    {move || {
                                        new_row_validations
                                            .get()
                                            .get(idx)
                                            .and_then(|row| row.description_error.clone())
                                            .map(|message| {
                                                view! { <span class="popup-error-indicator" title={message}>!</span> }
                                            })
                                    }}
                                </div>
                                <span class="popup-actions">
                                    {timer_buttons}
                                    {if let Some((href, message)) = row_link {
                                        view! {
                                            <a
                                                class="popup-link popup-link--commit"
                                                href={href}
                                                target="_blank"
                                                rel="noopener"
                                                title={if message.trim().is_empty() {
                                                    i18n.get_untracked().t(keys::OPEN_COMMIT_IN_BITBUCKET)
                                                } else {
                                                    message
                                                }}
                                            >
                                                <span class="popup-link-base">"\u{1F517}"</span>
                                                <span class="popup-link-badge">"C"</span>
                                            </a>
                                        }.into_any()
                                    } else {
                                        view! { <span class="popup-spacer"></span> }.into_any()
                                    }}
                                </span>
                            </div>
                        }
                    }).collect::<Vec<_>>()
                }}
            </div>

            <div class="popup-buttons">
                <span class="popup-buttons-left">
                    {commit_link_refs
                        .iter()
                        .map(|(href, message)| {
                            let href = href.clone();
                            let message = message.clone();
                            view! {
                                <a
                                    class="popup-link popup-link--commit"
                                    href={href}
                                    target="_blank"
                                    rel="noopener"
                                    title={if message.trim().is_empty() {
                                        i18n.get_untracked().t(keys::OPEN_COMMIT_IN_BITBUCKET)
                                    } else {
                                        message
                                    }}
                                >
                                    <span class="popup-link-base">"\u{1F517}"</span>
                                    <span class="popup-link-badge">"C"</span>
                                </a>
                            }
                        })
                        .collect::<Vec<_>>()}
                    {popup_pr_link.clone().map(|href| {
                        view! {
                            <a
                                class="popup-link popup-link--pr"
                                href={href}
                                target="_blank"
                                rel="noopener"
                                title=move || i18n.get().t(keys::OPEN_PR_IN_BITBUCKET)
                            >
                                <span class="popup-link-base">"\u{1F517}"</span>
                                <span class="popup-link-badge">"P"</span>
                            </a>
                        }
                    })}
                    {popup_test_link.clone().map(|href| {
                        view! {
                            <a
                                class="popup-link popup-link--test"
                                href={href}
                                target="_blank"
                                rel="noopener"
                                title=move || i18n.get().t(keys::OPEN_TEST_RESULTS_IN_JENKINS)
                            >
                                <span class="popup-link-base">"\u{1F517}"</span>
                                <span class="popup-link-badge">"T"</span>
                            </a>
                        }
                    })}
                </span>
            </div>
        </div>
        </div>
    }
}
