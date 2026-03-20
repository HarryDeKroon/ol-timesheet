use crate::components::popup_flush::{FlushLatch, use_popup_flush};
use crate::components::timer::{ProgressInfo, TimerId, TimerPhase, use_timer};
use crate::connection::use_connection;
use crate::formatting::{format_hours_long, parse_hours};
use crate::i18n::{I18n, keys};
use crate::model::WorklogEntry;
use chrono::NaiveDate;
use leptos::prelude::*;

use std::cell::Cell;
use std::rc::Rc;
use std::sync::{Arc, atomic::AtomicBool};

#[server(AddWorklog, "/api")]
pub async fn server_add_worklog(
    issue_key: String,
    date: NaiveDate,
    hours: f64,
    comment: String,
) -> Result<(), ServerFnError> {
    let (_, session) = crate::auth::current_user_session().await?;
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
) -> Result<(), ServerFnError> {
    let (_, session) = crate::auth::current_user_session().await?;
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
) -> Result<(), ServerFnError> {
    let (_, session) = crate::auth::current_user_session().await?;
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
    suggested_comment: Option<String>,
    is_git_log: bool,
    /// Whether this popup's date column is today (enables timer controls).
    #[prop(default = false)]
    is_today: bool,
    on_close: Callback<()>,
    on_changed: Callback<String>,
    #[prop(default = String::new())]
    site_url: String,
) -> impl IntoView {
    let i18n = use_context::<RwSignal<I18n>>().expect("I18n context");
    let conn = use_connection();
    let timer_mgr = use_timer();
    let flush_mgr = use_popup_flush();

    let issue_key_for_close = issue_key.clone();
    let date_for_close = date;

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
            ExistingEntry {
                id: e.id.clone(),
                hours_sig: RwSignal::new(hours_text.clone()),
                initial_hours: hours_text,
                comment_sig: RwSignal::new(display_comment.clone()),
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
    let new_entries: RwSignal<Vec<(RwSignal<String>, RwSignal<String>)>> = RwSignal::new(vec![(
        RwSignal::new(String::new()),
        RwSignal::new(if is_git_log {
            suggested_comment.unwrap_or_default()
        } else {
            String::new()
        }),
    )]);

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

    // ── Validation (Save enablement) ────────────────────────────────────
    let existing_for_validation = existing.clone();
    let save_enabled = Memo::new(move |_| {
        let w = i18n.get();
        let dec_sep = w.decimal_separator;
        let wl = w.t(keys::WEEK_ABBR);
        let dl = w.t(keys::DAY_ABBR);
        let hl = w.t(keys::HOUR_ABBR);
        let ml = w.t(keys::MINUTE_ABBR);

        let mut entry_hours: Vec<String> = Vec::new();
        let mut entry_comments: Vec<String> = Vec::new();

        for entry in existing_for_validation.iter() {
            if entry.deleted.get() {
                continue;
            }
            entry_hours.push(entry.hours_sig.get());
            let comment = if !entry.comment_html.is_empty() {
                entry.display_comment.clone()
            } else {
                entry.comment_sig.get()
            };
            entry_comments.push(comment);
        }

        let rows = new_entries.get();
        for (idx, (h_sig, c_sig)) in rows.iter().enumerate() {
            let h = h_sig.get();
            let c = c_sig.get();
            let is_last = idx == rows.len() - 1;
            if is_last && h.is_empty() && c.is_empty() {
                continue;
            }
            entry_hours.push(h);
            entry_comments.push(c);
        }

        let entry_count = entry_hours.len();

        let has_deletes = existing_for_validation.iter().any(|e| e.deleted.get());

        if entry_count == 0 {
            return has_deletes;
        }

        let all_hours_valid = entry_hours.iter().all(|h| {
            !h.is_empty()
                && parse_hours(
                    h,
                    hours_per_day,
                    hours_per_week,
                    dec_sep,
                    &wl,
                    &dl,
                    &hl,
                    &ml,
                )
                .is_some()
        });
        if !all_hours_valid {
            return false;
        }

        if entry_count > 1 && entry_comments.iter().any(|c| c.trim().is_empty()) {
            return false;
        }

        true
    });

    // ── Save handler ────────────────────────────────────────────────────
    let existing_for_save = existing.clone();
    let on_save = {
        let ik = issue_key.clone();
        let issue_key_for_stop = issue_key.clone();
        move |latch: Option<FlushLatch>| {
            let w = i18n.get_untracked();
            let dec_sep = w.decimal_separator;
            let wl = w.t(keys::WEEK_ABBR);
            let dl = w.t(keys::DAY_ABBR);
            let hl = w.t(keys::HOUR_ABBR);
            let ml = w.t(keys::MINUTE_ABBR);
            let ik = ik.clone();

            // Stop all timers for this popup on save
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
                for (ik, id) in deletes {
                    let _ = server_delete_worklog(ik, id).await;
                }
                for (ik, id, h, comment, adf) in updates {
                    let _ = server_update_worklog(ik, id, h, comment, adf).await;
                }
                for (ik, date, h, comment) in creates {
                    let _ = server_add_worklog(ik, date, h, comment).await;
                }
                conn.request_finished();
                // Signal the flush latch (if any) *before* refetch so that
                // callers waiting on the latch (e.g. language-switch reload)
                // can proceed now that the server state is up-to-date.
                if let Some(latch) = latch {
                    latch.arrive();
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
        let is_dirty: Rc<dyn Fn() -> bool> = Rc::new(move || {
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
            Rc::new(move || save_enabled.get_untracked())
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
        move || {
            timer_mgr.stop_all_for_popup(&ik, date_for_close);
            on_close.run(());
        }
    };

    // ── Keyboard shortcuts ──────────────────────────────────────────────
    let on_keydown = {
        let on_save = on_save.clone();
        let on_close_with_timers = on_close_with_timers.clone();
        move |ev: leptos::ev::KeyboardEvent| match ev.key().as_str() {
            "Enter" => {
                if save_enabled.get() {
                    on_save(None);
                }
            }
            "Escape" => {
                on_close_with_timers();
            }
            _ => {}
        }
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
        if !is_today {
            return view! { <span class="popup-spacer"></span> }.into_any();
        }

        let tid_for_phase = timer_id.clone();
        let tid_start = timer_id.clone();
        let tid_pause = timer_id.clone();
        let tid_resume = timer_id.clone();
        let on_play_pause = move |_| {
            let phase = timer_mgr.phase(&tid_for_phase);
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

    //
    view! {
         <div
            class="cell-popup"
            node_ref=popup_ref
            data-popup-id={popup_id.to_string()}
            style=move || pos_sig.get()
         >
            <div class="popup-draggable-title" on:mousedown=on_header_mousedown>
            <span class="popup-key">{issue_key}</span>
            <span class="popup-summary" title={issue_summary}>{issue_summary.clone()}</span>
            <span class="popup-date">{i18n.get_untracked().format_date(&date)}</span>
        </div>
        <div class="cell-popup-content" on:keydown=on_keydown tabindex="0">
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
                                            prop:value={move || hours_sig.get()}
                                            on:input=move |ev| hours_sig.set(event_target_value(&ev))
                                            placeholder={move || i18n.get().t(keys::HOURS)}
                                            disabled=move || !conn.is_available() || timer_mgr.is_active(&tid_for_disabled)
                                        />
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
                                            <div class="adf-preview" inner_html={comment_html.clone()}></div>
                                        }.into_any()
                                    } else {
                                        view! {
                                            <textarea
                                                class="popup-comment"
                                                prop:value={move || comment_sig.get()}
                                                on:input=move |ev| comment_sig.set(event_target_value(&ev))
                                                placeholder={move || i18n.get().t(keys::DESCRIPTION)}
                                                disabled=move || !conn.is_available()
                                                rows="1"
                                            />
                                        }.into_any()
                                    }}
                                    <span class="popup-actions">
                                        {timer_buttons}
                                        <a
                                            class="popup-worklog-link"
                                            href={link_href.clone()}
                                            target="_blank"
                                            rel="noopener"
                                            title=move || i18n.get().t(keys::OPEN_IN_JIRA)
                                        >
                                            "\u{1F517}"
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

                        view! {
                            <div class="popup-entry popup-new">
                                <div class="popup-hours-container">
                                    <input
                                        type="text"
                                        class="popup-hours"
                                        prop:value={move || hours_sig.get()}
                                        on:input=move |ev| hours_sig.set(event_target_value(&ev))
                                        on:blur=move |_| on_blur(idx)
                                        placeholder={move || i18n.get().t(keys::HOURS)}
                                        disabled=move || !conn.is_available() || timer_mgr.is_active(&tid_for_disabled)
                                    />
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
                                <textarea
                                    class="popup-comment popup-comment-new"
                                    prop:value={move || comment_sig.get()}
                                    on:input=move |ev| comment_sig.set(event_target_value(&ev))
                                    placeholder={move || i18n.get().t(keys::DESCRIPTION)}
                                    disabled=move || !conn.is_available()
                                    rows="1"
                                />
                                <span class="popup-actions">
                                    {timer_buttons}
                                    <span class="popup-spacer"></span>
                                </span>
                            </div>
                        }
                    }).collect::<Vec<_>>()
                }}
            </div>

            <div class="popup-buttons">
                <button
                    class="btn-ok"
                    on:click=move |_| on_save(None)
                    disabled=move || !save_enabled.get() || !conn.is_available()
                >
                    {move || i18n.get().t(keys::SAVE)}
                </button>
                <button class="btn-cancel" on:click=move |_| on_close_with_timers_for_btn()>
                    {move || i18n.get().t(keys::CLOSE)}
                </button>
            </div>
        </div>
        </div>
    }
}
