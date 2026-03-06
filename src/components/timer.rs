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

use leptos::prelude::*;
use std::collections::HashMap;

#[cfg(feature = "hydrate")]
const MINUTES_TO_MILLISECONDS: u32 = 60_000;
#[cfg(feature = "hydrate")]
const TIMER_INTERVAL: u32 = 5 * MINUTES_TO_MILLISECONDS;
#[cfg(feature = "hydrate")]
const TIMER_INTERVAL_SHORT: u32 = TIMER_INTERVAL >> 1;

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
}

/// Provide a [`TimerManager`] via Leptos context. Call once at app startup.
pub fn provide_timer_context() -> TimerManager {
    let mgr = TimerManager::new();
    provide_context(mgr);
    mgr
}

/// Obtain the [`TimerManager`] from context.
pub fn use_timer() -> TimerManager {
    use_context::<TimerManager>().expect("TimerManager context not provided")
}

// ---------------------------------------------------------------------------
// Browser helpers (hydrate-only)
// ---------------------------------------------------------------------------

/// Schedule a JS `setTimeout` that, when it fires:
/// 1. Bumps the hours signal by 5 minutes.
/// 2. Schedules the next 5-minute interval.
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

    let handle = web_sys::window()
        .expect("no window")
        .set_timeout_with_callback_and_timeout_and_arguments_0(
            cb.as_ref().unchecked_ref(),
            delay_ms as i32,
        )
        .unwrap_or(0);

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
    use crate::formatting::{format_hours_long, parse_hours};
    use crate::i18n::{I18n, keys};

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

    // Bump the duration by 5 minutes (= 5/60 hours).
    let five_min_hours: f64 = 5.0 / 60.0;

    // Resolve i18n labels early so we can use them for both parsing and formatting.
    let i18n = use_context::<RwSignal<I18n>>()
        .map(|s| s.get_untracked())
        .unwrap_or_default();
    let wl = i18n.t(keys::WEEK_ABBR);
    let dl = i18n.t(keys::DAY_ABBR);
    let hl = i18n.t(keys::HOUR_ABBR);
    let ml = i18n.t(keys::MINUTE_ABBR);

    // Guard against disposed signal (popup closed between scheduling and firing).
    let current_text = match hours_signal.try_get_untracked() {
        Some(t) => t,
        None => {
            leptos::logging::log!(
                "interval fire: hours_signal disposed for {:?}, skipping.",
                id
            );
            return;
        }
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
    let new_hours = current_hours + five_min_hours;

    let new_text = format_hours_long(new_hours, hours_per_day, hours_per_week, &wl, &dl, &hl, &ml);
    // Guard: signal may have been disposed between the get and the set.
    // `try_set` returns `Some(value)` when the signal is disposed (set failed),
    // and `None` on success.
    if hours_signal.try_set(new_text).is_some() {
        leptos::logging::log!(
            "interval fire: hours_signal disposed during set for {:?}, skipping.",
            id
        );
        return;
    }

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
