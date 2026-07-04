//! Time-tracking timer state management.
//!
//! Provides a global `TimerManager` (stored in Leptos context) that tracks
//! per-row timer state. Only one timer can be *running* at a time; starting
//! a new timer automatically pauses any previously running one.
//!
//! ## Timer interval logic
//!
//! 1. **Start** — parse the current duration, begin a browser `setTimeout`
//!    for 2.5 minutes (150 000 ms).
//! 2. After 2.5 min the duration is bumped by 5 min and a new 5-min timer
//!    starts (300 000 ms). This repeats indefinitely.
//! 3. **Pause** — record the remaining time of the current interval. The
//!    duration is **not** updated.
//! 4. **Resume** — start a timer for the remaining time. When it fires the
//!    duration is bumped by 5 min and the cycle continues with 5-min
//!    intervals.
//! 5. **Stop** — cancel the running/paused timer. The duration is **not**
//!    updated (the previous interval already accounted for it).

use cfg_if::cfg_if;
use leptos::prelude::*;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

cfg_if! {
    if #[cfg(feature = "hydrate")] {
    const MINUTES_TO_MILLISECONDS: u32 = 60_000;
const TIMER_INTERVAL: u32 = 5 * MINUTES_TO_MILLISECONDS;
const TIMER_INTERVAL_SHORT: u32 = TIMER_INTERVAL >> 1;
    }
}
#[cfg(feature = "hydrate")]
const TIMER_STORAGE_KEY: &str = "timesheet_timers.yaml";

// ---------------------------------------------------------------------------
// TimerId — uniquely identifies a timer row (issue_key + worklog-row index)
// ---------------------------------------------------------------------------

/// Identifies a single timer row inside a cell popup.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct TimerId {
    pub issue_key: String,
    pub date: chrono::NaiveDate,
    /// Index of the worklog entry row inside the popup (0-based).
    pub row_index: usize,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub enum PersistedTimerPhase {
    Running,
    Paused,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct PersistedTimerState {
    pub phase: PersistedTimerPhase,
    pub is_first_interval: bool,
    #[serde(alias = "interval_ms")]
    pub remaining_ms: u32,
    pub elapsed_ms: u32,
    pub generation: u32,
    #[serde(default)]
    pub snapshot_at_epoch_ms: Option<i64>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct PersistedTimerRow {
    #[serde(default)]
    pub row_index: usize,
    pub worklog_id: Option<String>,
    pub hours_text: String,
    pub comment_text: String,
    pub timer_state: PersistedTimerState,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct PersistedTimerPopup {
    pub issue_key: String,
    pub issue_summary: String,
    pub date: chrono::NaiveDate,
    #[serde(default)]
    pub suggested_comment: Option<String>,
    #[serde(default)]
    pub is_git_log: bool,
    #[serde(default)]
    pub is_weekend: bool,
    #[serde(default)]
    pub position_style: Option<String>,
    #[serde(default)]
    pub rows: Vec<PersistedTimerRow>,
}

#[cfg(feature = "hydrate")]
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
struct PersistedTimerFile {
    #[serde(default)]
    popups: Vec<PersistedTimerPopup>,
}

// ---------------------------------------------------------------------------
// TimerPhase — the state-machine for a single timer
// ---------------------------------------------------------------------------

/// Current phase of an individual timer.
#[derive(Clone, Debug, PartialEq)]
pub enum TimerPhase {
    /// Timer is actively counting down.
    Running,
    /// Timer is paused with `remaining_ms` left in the current interval.
    Paused { remaining_ms: u32 },
    /// Timer has been stopped (terminal state).
    Stopped,
}

// ---------------------------------------------------------------------------
// ProgressInfo — data the UI needs to drive the CSS animation
// ---------------------------------------------------------------------------

/// Progress bar parameters exposed to the UI layer.
///
/// The UI uses these to set CSS custom properties and classes on the
/// progress-bar element so that a pure-CSS `@keyframes` animation
/// handles the smooth width transition without any JS intervals.
#[derive(Clone, Debug)]
pub struct ProgressInfo {
    /// Whether this is the first interval (short) or a subsequent one.
    pub is_first_interval: bool,
    /// Milliseconds already elapsed before the current run segment
    /// (accumulated across pauses). Used as a negative
    /// `animation-delay` so the animation starts at the right offset.
    pub offset_ms: u32,
    /// `true` when the timer is running, `false` when paused.
    pub running: bool,
    /// Monotonically increasing counter, incremented each time a new
    /// interval starts.  Used as a DOM key so the browser creates a
    /// fresh element and restarts the CSS animation.
    pub generation: u32,
}

// ---------------------------------------------------------------------------
// TimerEntry — all bookkeeping for one timer
// ---------------------------------------------------------------------------

/// Internal bookkeeping for a single timer row.
#[derive(Clone, Debug)]
pub struct TimerEntry {
    pub phase: TimerPhase,
    /// Whether this is the very first interval (2.5 min) or a subsequent
    /// 5-min interval.
    pub is_first_interval: bool,
    /// Duration of the current (or last) interval in milliseconds.
    pub interval_ms: u32,
    /// JS timeout handle returned by `setTimeout`, used for cancellation.
    /// `0` means no active timeout.
    pub timeout_handle: i32,
    /// Timestamp (`Performance.now()`) when the current interval was
    /// started. Used to compute the true remaining time on pause.
    pub interval_started_at: f64,
    /// The signal that holds the hours text for this row. When an interval
    /// fires we bump the value by 5 minutes (≈ 0.0833… h).
    pub hours_signal: RwSignal<String>,
    /// Hours-per-day setting, needed for `parse_hours` / `format_hours_long`.
    pub hours_per_day: f64,
    /// Hours-per-week setting.
    pub hours_per_week: f64,
    /// The total duration of the current interval (before any pause).
    /// Used together with `accumulated_ms` to drive the CSS animation.
    pub total_interval_ms: u32,
    /// Milliseconds elapsed in previous run segments before the last pause.
    pub accumulated_ms: u32,
    /// Monotonically increasing counter, bumped on each new interval.
    pub generation: u32,
}

/// Return the current high-resolution timestamp in milliseconds via
/// `Performance.now()`, or `0.0` if unavailable.
#[cfg(feature = "hydrate")]
fn now_ms() -> f64 {
    web_sys::window()
        .and_then(|w| w.performance())
        .map(|p| p.now())
        .unwrap_or(0.0)
}

#[cfg(feature = "hydrate")]
fn now_epoch_ms() -> i64 {
    Utc::now().timestamp_millis()
}

#[cfg(feature = "hydrate")]
fn timer_storage() -> Option<web_sys::Storage> {
    let Some(window) = web_sys::window() else {
        log::warn!("timer persistence: window unavailable");
        return None;
    };
    match window.local_storage() {
        Ok(Some(storage)) => Some(storage),
        Ok(None) => {
            log::warn!("timer persistence: localStorage not available");
            None
        }
        Err(err) => {
            log::warn!("timer persistence: localStorage access error: {:?}", err);
            None
        }
    }
}

#[cfg(feature = "hydrate")]
fn read_persisted_timer_file() -> PersistedTimerFile {
    let Some(storage) = timer_storage() else {
        return PersistedTimerFile::default();
    };
    let Some(raw) = storage.get_item(TIMER_STORAGE_KEY).ok().flatten() else {
        return PersistedTimerFile::default();
    };

    match serde_yaml::from_str::<PersistedTimerFile>(&raw) {
        Ok(file) => file,
        Err(err) => {
            log::warn!("failed to parse persisted timers from local storage: {err}");
            PersistedTimerFile::default()
        }
    }
}

#[cfg(feature = "hydrate")]
fn write_persisted_timer_file(file: &PersistedTimerFile) {
    let Some(storage) = timer_storage() else {
        return;
    };

    if file.popups.is_empty() {
        if let Err(err) = storage.remove_item(TIMER_STORAGE_KEY) {
            log::warn!(
                "timer persistence: failed removing key {}: {:?}",
                TIMER_STORAGE_KEY,
                err
            );
        }
        return;
    }

    match serde_yaml::to_string(file) {
        Ok(raw) => {
            if let Err(err) = storage.set_item(TIMER_STORAGE_KEY, &raw) {
                log::warn!(
                    "timer persistence: failed writing key {}: {:?}",
                    TIMER_STORAGE_KEY,
                    err
                );
            }
        }
        Err(err) => {
            log::warn!("failed to serialize persisted timers to local storage: {err}");
        }
    }
}

#[cfg(feature = "hydrate")]
pub fn load_persisted_timer_popups() -> Vec<PersistedTimerPopup> {
    read_persisted_timer_file().popups
}

#[cfg(not(feature = "hydrate"))]
pub fn load_persisted_timer_popups() -> Vec<PersistedTimerPopup> {
    Vec::new()
}

#[cfg(feature = "hydrate")]
pub fn save_persisted_timer_popup(popup: PersistedTimerPopup) {
    let mut file = read_persisted_timer_file();
    if let Some(existing) = file
        .popups
        .iter_mut()
        .find(|p| p.issue_key == popup.issue_key && p.date == popup.date)
    {
        *existing = popup;
    } else {
        file.popups.push(popup);
    }
    write_persisted_timer_file(&file);
}

#[cfg(not(feature = "hydrate"))]
pub fn save_persisted_timer_popup(_popup: PersistedTimerPopup) {}

#[cfg(feature = "hydrate")]
pub fn upsert_persisted_timer_row(
    issue_key: &str,
    issue_summary: &str,
    date: chrono::NaiveDate,
    suggested_comment: Option<String>,
    is_git_log: bool,
    is_weekend: bool,
    position_style: Option<String>,
    row: PersistedTimerRow,
) {
    let mut file = read_persisted_timer_file();
    if let Some(popup) = file
        .popups
        .iter_mut()
        .find(|p| p.issue_key == issue_key && p.date == date)
    {
        popup.issue_summary = issue_summary.to_string();
        popup.suggested_comment = suggested_comment;
        popup.is_git_log = is_git_log;
        popup.is_weekend = is_weekend;
        popup.position_style = position_style;
        popup.rows.retain(|existing| {
            if existing.row_index == row.row_index {
                return false;
            }
            if let Some(row_id) = &row.worklog_id {
                return existing.worklog_id.as_ref() != Some(row_id);
            }
            true
        });
        popup.rows.push(row);
    } else {
        file.popups.push(PersistedTimerPopup {
            issue_key: issue_key.to_string(),
            issue_summary: issue_summary.to_string(),
            date,
            suggested_comment,
            is_git_log,
            is_weekend,
            position_style,
            rows: vec![row],
        });
    }
    write_persisted_timer_file(&file);
}

#[cfg(not(feature = "hydrate"))]
pub fn upsert_persisted_timer_row(
    _issue_key: &str,
    _issue_summary: &str,
    _date: chrono::NaiveDate,
    _suggested_comment: Option<String>,
    _is_git_log: bool,
    _is_weekend: bool,
    _position_style: Option<String>,
    _row: PersistedTimerRow,
) {
}

#[cfg(feature = "hydrate")]
pub fn ensure_timer_storage_initialized() {
    let Some(storage) = timer_storage() else {
        return;
    };
    if storage.get_item(TIMER_STORAGE_KEY).ok().flatten().is_none() {
        if let Err(err) = storage.set_item(TIMER_STORAGE_KEY, "popups: []\n") {
            log::warn!(
                "timer persistence: failed initializing key {}: {:?}",
                TIMER_STORAGE_KEY,
                err
            );
        }
    }
}

#[cfg(not(feature = "hydrate"))]
pub fn ensure_timer_storage_initialized() {}

#[cfg(feature = "hydrate")]
pub fn remove_persisted_timer_popup(issue_key: &str, date: chrono::NaiveDate) {
    let mut file = read_persisted_timer_file();
    file.popups
        .retain(|popup| popup.issue_key != issue_key || popup.date != date);
    write_persisted_timer_file(&file);
}

#[cfg(not(feature = "hydrate"))]
pub fn remove_persisted_timer_popup(_issue_key: &str, _date: chrono::NaiveDate) {}

// ---------------------------------------------------------------------------
// TimerManager — the global singleton
// ---------------------------------------------------------------------------

/// Global timer manager stored in Leptos context.
///
/// Wraps a reactive signal around a `HashMap<TimerId, TimerEntry>` so that
/// the UI can read individual timer phases reactively.
#[derive(Clone, Copy)]
pub struct TimerManager {
    inner: RwSignal<HashMap<TimerId, TimerEntry>>,
}

impl TimerManager {
    /// Create a new, empty manager.
    pub fn new() -> Self {
        Self {
            inner: RwSignal::new(HashMap::new()),
        }
    }

    // -- Queries -----------------------------------------------------------

    /// Return the current phase for a timer, or `None` if no timer exists.
    pub fn phase(&self, id: &TimerId) -> Option<TimerPhase> {
        self.inner.with(|map| map.get(id).map(|e| e.phase.clone()))
    }

    /// `true` if the timer identified by `id` is in the `Running` phase.
    pub fn is_running(&self, id: &TimerId) -> bool {
        matches!(self.phase(id), Some(TimerPhase::Running))
    }

    /// `true` if the timer identified by `id` is in the `Paused` phase.
    pub fn is_paused(&self, id: &TimerId) -> bool {
        matches!(self.phase(id), Some(TimerPhase::Paused { .. }))
    }

    /// `true` if a timer exists and is either Running or Paused.
    pub fn is_active(&self, id: &TimerId) -> bool {
        self.is_running(id) || self.is_paused(id)
    }

    /// Return the `TimerId` of the currently running timer, if any.
    pub fn running_id(&self) -> Option<TimerId> {
        self.inner.with_untracked(|map| {
            map.iter()
                .find(|(_, e)| e.phase == TimerPhase::Running)
                .map(|(id, _)| id.clone())
        })
    }

    /// Return the progress-bar parameters for a timer, if it exists and
    /// is active (Running or Paused).
    ///
    /// The UI uses these values to set CSS custom properties
    /// (`--timer-duration`, `--timer-offset`) and toggle the
    /// `timer-progress-paused` class.
    pub fn progress_info(&self, id: &TimerId) -> Option<ProgressInfo> {
        self.inner.with(|map| {
            map.get(id).and_then(|e| match &e.phase {
                TimerPhase::Running => Some(ProgressInfo {
                    is_first_interval: e.is_first_interval,
                    offset_ms: e.accumulated_ms,
                    running: true,
                    generation: e.generation,
                }),
                TimerPhase::Paused { .. } => Some(ProgressInfo {
                    is_first_interval: e.is_first_interval,
                    offset_ms: e.accumulated_ms,
                    running: false,
                    generation: e.generation,
                }),
                TimerPhase::Stopped => None,
            })
        })
    }

    #[cfg(feature = "hydrate")]
    pub fn persisted_state(&self, id: &TimerId) -> Option<PersistedTimerState> {
        self.inner.with_untracked(|map| {
            let entry = map.get(id)?;
            let elapsed_ms = match entry.phase {
                TimerPhase::Running => {
                    let segment_elapsed = (now_ms() - entry.interval_started_at).max(0.0) as u32;
                    (entry.accumulated_ms + segment_elapsed).min(entry.total_interval_ms)
                }
                TimerPhase::Paused { .. } => entry.accumulated_ms.min(entry.total_interval_ms),
                TimerPhase::Stopped => return None,
            };

            let phase = match entry.phase {
                TimerPhase::Running => PersistedTimerPhase::Running,
                TimerPhase::Paused { .. } => PersistedTimerPhase::Paused,
                TimerPhase::Stopped => return None,
            };

            let remaining_ms = match &entry.phase {
                TimerPhase::Running => {
                    let segment_elapsed = (now_ms() - entry.interval_started_at).max(0.0) as u32;
                    entry.interval_ms.saturating_sub(segment_elapsed).max(1)
                }
                TimerPhase::Paused { remaining_ms } => *remaining_ms,
                TimerPhase::Stopped => return None,
            };

            Some(PersistedTimerState {
                phase,
                is_first_interval: entry.is_first_interval,
                remaining_ms,
                elapsed_ms,
                generation: entry.generation,
                snapshot_at_epoch_ms: matches!(&entry.phase, TimerPhase::Running)
                    .then_some(now_epoch_ms()),
            })
        })
    }

    #[cfg(feature = "hydrate")]
    pub fn restore_persisted_state(
        &self,
        id: TimerId,
        hours_signal: RwSignal<String>,
        hours_per_day: f64,
        hours_per_week: f64,
        decimal_sep: char,
        state: PersistedTimerState,
    ) {
        let mut remaining_ms = state.remaining_ms.max(1);
        let mut elapsed_ms = state.elapsed_ms;
        let mut generation = state.generation;
        let mut is_first_interval = state.is_first_interval;
        let mut completed_intervals = 0u32;
        let is_running = matches!(&state.phase, PersistedTimerPhase::Running);

        if is_running {
            let mut extra_elapsed = state
                .snapshot_at_epoch_ms
                .map(|snapshot| (now_epoch_ms() - snapshot).max(0) as u32)
                .unwrap_or(0);

            if extra_elapsed < remaining_ms {
                remaining_ms -= extra_elapsed;
                elapsed_ms += extra_elapsed;
            } else {
                extra_elapsed = extra_elapsed.saturating_sub(remaining_ms);
                completed_intervals += 1;
                generation += 1;
                is_first_interval = false;
                remaining_ms = TIMER_INTERVAL;
                elapsed_ms = 0;

                if extra_elapsed >= TIMER_INTERVAL {
                    let extra_intervals = extra_elapsed / TIMER_INTERVAL;
                    completed_intervals += extra_intervals;
                    generation += extra_intervals;
                    extra_elapsed %= TIMER_INTERVAL;
                }

                if extra_elapsed > 0 {
                    remaining_ms = TIMER_INTERVAL.saturating_sub(extra_elapsed).max(1);
                    elapsed_ms = extra_elapsed;
                }
            }
        }

        if completed_intervals > 0 {
            add_completed_intervals_to_hours(
                hours_signal,
                completed_intervals,
                hours_per_day,
                hours_per_week,
                decimal_sep,
            );
        }

        let total_interval_ms = if completed_intervals > 0 || !is_first_interval {
            TIMER_INTERVAL
        } else {
            remaining_ms.saturating_add(elapsed_ms).max(1)
        };

        let timeout_handle = if is_running {
            schedule_interval(
                self.inner,
                id.clone(),
                remaining_ms,
                hours_signal,
                hours_per_day,
                hours_per_week,
                decimal_sep,
            )
        } else {
            0
        };

        let phase = if is_running {
            TimerPhase::Running
        } else {
            TimerPhase::Paused { remaining_ms }
        };

        self.inner.update(|map| {
            map.insert(
                id,
                TimerEntry {
                    phase,
                    is_first_interval,
                    interval_ms: remaining_ms,
                    timeout_handle,
                    interval_started_at: now_ms(),
                    hours_signal,
                    hours_per_day,
                    hours_per_week,
                    total_interval_ms,
                    accumulated_ms: elapsed_ms.min(total_interval_ms),
                    generation,
                },
            );
        });
    }

    // -- Mutations (client-only implementations) ---------------------------

    /// Start a new timer for the given row.
    ///
    /// * If another timer is currently running it is **paused** first.
    /// * The first interval is 2.5 minutes (150 000 ms).
    ///
    /// `hours_signal` is the `RwSignal<String>` that holds the duration text
    /// for this popup row. It will be mutated when intervals fire.
    #[cfg(feature = "hydrate")]
    pub fn start(
        &self,
        id: TimerId,
        hours_signal: RwSignal<String>,
        hours_per_day: f64,
        hours_per_week: f64,
        decimal_sep: char,
    ) {
        // Pause any currently running timer.
        if let Some(running) = self.running_id() {
            if running != id {
                self.pause(&running);
            }
        }

        let first_interval_ms: u32 = TIMER_INTERVAL_SHORT;

        let handle = schedule_interval(
            self.inner,
            id.clone(),
            first_interval_ms,
            hours_signal,
            hours_per_day,
            hours_per_week,
            decimal_sep,
        );

        let started_at = now_ms();
        self.inner.update(|map| {
            map.insert(
                id,
                TimerEntry {
                    phase: TimerPhase::Running,
                    is_first_interval: true,
                    interval_ms: first_interval_ms,
                    timeout_handle: handle,
                    interval_started_at: started_at,
                    hours_signal,
                    hours_per_day,
                    hours_per_week,
                    total_interval_ms: first_interval_ms,
                    accumulated_ms: 0,
                    generation: 0,
                },
            );
        });
    }

    /// Pause the timer identified by `id`.
    ///
    /// Records the remaining time so that `resume` can pick up where we left
    /// off.
    #[cfg(feature = "hydrate")]
    pub fn pause(&self, id: &TimerId) {
        let paused_at = now_ms();
        self.inner.update(|map| {
            if let Some(entry) = map.get_mut(id) {
                if entry.phase != TimerPhase::Running {
                    return;
                }
                // Cancel the pending JS timeout.
                cancel_timeout(entry.timeout_handle);

                // Compute true remaining time using the high-resolution
                // timestamp recorded when the interval was started.
                let elapsed = (paused_at - entry.interval_started_at).max(0.0) as u32;
                let remaining = entry.interval_ms.saturating_sub(elapsed).max(1);
                // Accumulate elapsed time for correct progress after resume.
                entry.accumulated_ms += elapsed;
                entry.phase = TimerPhase::Paused {
                    remaining_ms: remaining,
                };
                entry.timeout_handle = 0;
            }
        });
    }

    /// Resume a paused timer.
    #[cfg(feature = "hydrate")]
    pub fn resume(&self, id: &TimerId, decimal_sep: char) {
        let info = self.inner.with_untracked(|map| {
            map.get(id).and_then(|entry| {
                if let TimerPhase::Paused { remaining_ms } = &entry.phase {
                    Some((
                        *remaining_ms,
                        entry.hours_signal,
                        entry.hours_per_day,
                        entry.hours_per_week,
                    ))
                } else {
                    None
                }
            })
        });

        let Some((remaining_ms, hours_signal, hpd, hpw)) = info else {
            return;
        };

        // Pause any other running timer first.
        if let Some(running) = self.running_id() {
            if running != *id {
                self.pause(&running);
            }
        }

        let handle = schedule_interval(
            self.inner,
            id.clone(),
            remaining_ms,
            hours_signal,
            hpd,
            hpw,
            decimal_sep,
        );

        let started_at = now_ms();
        self.inner.update(|map| {
            if let Some(entry) = map.get_mut(id) {
                entry.phase = TimerPhase::Running;
                entry.interval_ms = remaining_ms;
                entry.timeout_handle = handle;
                entry.interval_started_at = started_at;
            }
        });
    }

    /// Stop a timer (terminal). Does **not** update the duration.
    #[cfg(feature = "hydrate")]
    pub fn stop(&self, id: &TimerId) {
        self.inner.update(|map| {
            if let Some(entry) = map.get_mut(id) {
                if let TimerPhase::Running = &entry.phase {
                    cancel_timeout(entry.timeout_handle);
                }
                entry.phase = TimerPhase::Stopped;
                entry.timeout_handle = 0;
            }
        });
    }

    /// Stop and remove **all** timers. Called when a popup is closed.
    #[cfg(feature = "hydrate")]
    pub fn stop_all_for_popup(&self, issue_key: &str, date: chrono::NaiveDate) {
        self.inner.update(|map| {
            let ids_to_remove: Vec<TimerId> = map
                .keys()
                .filter(|k| k.issue_key == issue_key && k.date == date)
                .cloned()
                .collect();
            for id in &ids_to_remove {
                if let Some(entry) = map.get(id) {
                    if entry.phase == TimerPhase::Running {
                        cancel_timeout(entry.timeout_handle);
                    }
                }
                map.remove(id);
            }
        });
    }

    /// Stop and remove all timers globally.
    #[cfg(feature = "hydrate")]
    pub fn stop_all(&self) {
        self.inner.update(|map| {
            for entry in map.values() {
                if entry.phase == TimerPhase::Running {
                    cancel_timeout(entry.timeout_handle);
                }
            }
            map.clear();
        });
    }

    // -- No-op stubs for SSR -----------------------------------------------

    #[cfg(not(feature = "hydrate"))]
    pub fn start(
        &self,
        _id: TimerId,
        _hours_signal: RwSignal<String>,
        _hours_per_day: f64,
        _hours_per_week: f64,
        _decimal_sep: char,
    ) {
    }

    #[cfg(not(feature = "hydrate"))]
    pub fn pause(&self, _id: &TimerId) {}

    #[cfg(not(feature = "hydrate"))]
    pub fn resume(&self, _id: &TimerId, _decimal_sep: char) {}

    #[cfg(not(feature = "hydrate"))]
    pub fn stop(&self, _id: &TimerId) {}

    #[cfg(not(feature = "hydrate"))]
    pub fn stop_all_for_popup(&self, _issue_key: &str, _date: chrono::NaiveDate) {}

    #[cfg(not(feature = "hydrate"))]
    pub fn stop_all(&self) {}

    #[cfg(not(feature = "hydrate"))]
    pub fn persisted_state(&self, _id: &TimerId) -> Option<PersistedTimerState> {
        None
    }

    #[cfg(not(feature = "hydrate"))]
    pub fn restore_persisted_state(
        &self,
        _id: TimerId,
        _hours_signal: RwSignal<String>,
        _hours_per_day: f64,
        _hours_per_week: f64,
        _decimal_sep: char,
        _state: PersistedTimerState,
    ) {
    }
}

/// Provide a [`TimerManager`] via Leptos context. Call once at app startup.
pub fn provide_timer_context() -> TimerManager {
    let mgr = TimerManager::new();
    provide_context(mgr);
    mgr
}

/// Obtain the [`TimerManager`] from context.
pub fn use_timer() -> TimerManager {
    use_context::<TimerManager>().unwrap_or_else(|| {
        log::error!("TimerManager context not provided, using fallback manager");
        TimerManager::new()
    })
}

// ---------------------------------------------------------------------------
// Browser helpers (hydrate-only)
// ---------------------------------------------------------------------------

/// Schedule a JS `setTimeout` that, when it fires:
/// 1. Bumps the hours signal by 5 minutes.
/// 2. Schedules the next 5-minute interval.
#[cfg(feature = "hydrate")]
fn add_completed_intervals_to_hours(
    hours_signal: RwSignal<String>,
    completed_intervals: u32,
    hours_per_day: f64,
    hours_per_week: f64,
    decimal_sep: char,
) {
    use crate::formatting::{format_hours_long, parse_hours};
    use crate::i18n::{I18n, keys};

    if completed_intervals == 0 {
        return;
    }

    let i18n = use_context::<RwSignal<I18n>>()
        .map(|s| s.get_untracked())
        .unwrap_or_default();
    let wl = i18n.t(keys::WEEK_ABBR);
    let dl = i18n.t(keys::DAY_ABBR);
    let hl = i18n.t(keys::HOUR_ABBR);
    let ml = i18n.t(keys::MINUTE_ABBR);

    let Some(current_text) = hours_signal.try_get_untracked() else {
        return;
    };

    let current_hours = parse_hours(
        &current_text,
        hours_per_day,
        hours_per_week,
        decimal_sep,
        &wl,
        &dl,
        &hl,
        &ml,
    )
    .unwrap_or(0.0);
    let added_hours = completed_intervals as f64 * (5.0 / 60.0);
    let new_text = format_hours_long(
        current_hours + added_hours,
        hours_per_day,
        hours_per_week,
        &wl,
        &dl,
        &hl,
        &ml,
    );

    if hours_signal.try_set(new_text).is_some() {
        log::warn!("failed to restore timer hours because signal was disposed");
    }
}

#[cfg(feature = "hydrate")]
fn schedule_interval(
    inner: RwSignal<HashMap<TimerId, TimerEntry>>,
    id: TimerId,
    delay_ms: u32,
    hours_signal: RwSignal<String>,
    hours_per_day: f64,
    hours_per_week: f64,
    decimal_sep: char,
) -> i32 {
    use wasm_bindgen::JsCast;
    use wasm_bindgen::prelude::*;

    leptos::logging::log!("schedule_interval: id={:?} delay_ms={}", id, delay_ms);

    let id_clone = id.clone();
    let cb = Closure::once(Box::new(move || {
        on_interval_fire(
            inner,
            id_clone,
            hours_signal,
            hours_per_day,
            hours_per_week,
            decimal_sep,
        );
    }) as Box<dyn FnOnce()>);

    let handle = if let Some(window) = web_sys::window() {
        window
            .set_timeout_with_callback_and_timeout_and_arguments_0(
                cb.as_ref().unchecked_ref(),
                delay_ms as i32,
            )
            .unwrap_or(0)
    } else {
        0
    };

    cb.forget();
    handle
}

/// Called when a `setTimeout` fires. Bumps the duration by 5 min and
/// schedules the next 5-min interval.
#[cfg(feature = "hydrate")]
fn on_interval_fire(
    inner: RwSignal<HashMap<TimerId, TimerEntry>>,
    id: TimerId,
    hours_signal: RwSignal<String>,
    hours_per_day: f64,
    hours_per_week: f64,
    decimal_sep: char,
) {
    leptos::logging::log!("interval elapsed: id={:?}.", id);

    // If the timer was removed (e.g. popup closed), the hours_signal is
    // disposed.  Bail out before touching any signal to avoid a panic.
    let still_exists = inner
        .try_with_untracked(|map| map.contains_key(&id))
        .unwrap_or(false);
    if !still_exists {
        leptos::logging::log!("interval fire: timer {:?} no longer in map, skipping.", id);
        return;
    }

    add_completed_intervals_to_hours(hours_signal, 1, hours_per_day, hours_per_week, decimal_sep);

    // Check if this timer is still Running before scheduling the next tick.
    // This runs inside a setTimeout callback, outside any reactive tracking
    // context, so we must use with_untracked.
    let still_running = inner.with_untracked(|map| {
        map.get(&id)
            .map(|e| e.phase == TimerPhase::Running)
            .unwrap_or(false)
    });

    if !still_running {
        return;
    }

    // Schedule the next 5-minute interval.
    let next_interval_ms: u32 = TIMER_INTERVAL;

    let handle = schedule_interval(
        inner,
        id.clone(),
        next_interval_ms,
        hours_signal,
        hours_per_day,
        hours_per_week,
        decimal_sep,
    );

    let started_at = now_ms();
    inner.update(|map| {
        if let Some(entry) = map.get_mut(&id) {
            entry.is_first_interval = false;
            entry.interval_ms = next_interval_ms;
            entry.timeout_handle = handle;
            entry.interval_started_at = started_at;

            // Reset progress for the new interval.
            entry.total_interval_ms = next_interval_ms;
            entry.accumulated_ms = 0;
            entry.generation += 1;
        }
    });
}

/// Cancel a JS timeout by handle.
#[cfg(feature = "hydrate")]
fn cancel_timeout(handle: i32) {
    if handle != 0 {
        if let Some(window) = web_sys::window() {
            window.clear_timeout_with_handle(handle);
        }
    }
}
