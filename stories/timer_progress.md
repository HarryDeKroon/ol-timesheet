# Timer Progress Bar

Add a visual indicator of how much time a timer has left before the next
5-minute duration bump. A thin progress bar animates below the hours
`<input>`, growing from 0 % to 100 % over the current interval.

## Design

Instead of polling from JavaScript (e.g. a `setInterval` that updates a
reactive signal 100 times per interval), the progress bar is driven
entirely by a **CSS `@keyframes` animation**. The Rust/WASM side only
needs to pass two CSS custom properties and toggle a class:

| CSS custom property | Meaning                                                                                                       |
| ------------------- | ------------------------------------------------------------------------------------------------------------- |
| `--timer-duration`  | Total interval length in ms (e.g. `150000ms` or `300000ms`)                                                   |
| `--timer-offset`    | Negative `animation-delay` representing time already elapsed before the current run segment (e.g. `-75000ms`) |

| CSS class                | Effect                                                                         |
| ------------------------ | ------------------------------------------------------------------------------ |
| `timer-progress-running` | Plays the animation (`animation-play-state: running`)                          |
| `timer-progress-paused`  | Freezes the animation at its current position (`animation-play-state: paused`) |

**Benefits over JS-interval approach:**

- Zero reactive re-renders while the timer is running — the browser's
  compositor handles the smooth width transition.
- No `setInterval` handle to manage, schedule, or cancel.
- Pause/resume is trivial: toggle `animation-play-state`.
- Simpler code in every lifecycle method.

---

## 1. Update `TimerEntry` (`src/components/timer.rs`)

Add two fields to `TimerEntry` that track the full interval duration and
the time accumulated before the last pause. These are read by the UI to
set the CSS custom properties.

```rust src/components/timer.rs
pub struct TimerEntry {
    // ... existing fields ...

    /// The total duration of the current interval (before any pause).
    /// Used together with `accumulated_ms` to drive the CSS animation.
    pub total_interval_ms: u32,
    /// Milliseconds elapsed in previous run segments before the last pause.
    pub accumulated_ms: u32,
}
```

## 2. Add `ProgressInfo` and `progress_info()` (`src/components/timer.rs`)

A simple data struct carries the information the UI needs to configure
the CSS animation. `TimerManager::progress_info()` reads the entry's
phase and returns `Some(ProgressInfo)` for Running/Paused timers, or
`None` for stopped/absent ones.

```rust src/components/timer.rs
/// Progress bar parameters exposed to the UI layer.
#[derive(Clone, Debug)]
pub struct ProgressInfo {
    /// Total duration of the current interval in milliseconds.
    pub duration_ms: u32,
    /// Milliseconds already elapsed before the current run segment
    /// (accumulated across pauses). Used as a negative
    /// `animation-delay` so the animation starts at the right offset.
    pub offset_ms: u32,
    /// `true` when the timer is running, `false` when paused.
    pub running: bool,
}
```

```rust src/components/timer.rs
    pub fn progress_info(&self, id: &TimerId) -> Option<ProgressInfo> {
        self.inner.with(|map| {
            map.get(id).and_then(|e| match &e.phase {
                TimerPhase::Running => Some(ProgressInfo {
                    duration_ms: e.total_interval_ms,
                    offset_ms: e.accumulated_ms,
                    running: true,
                }),
                TimerPhase::Paused { .. } => Some(ProgressInfo {
                    duration_ms: e.total_interval_ms,
                    offset_ms: e.accumulated_ms,
                    running: false,
                }),
                TimerPhase::Stopped => None,
            })
        })
    }
```

## 3. Update lifecycle methods (`src/components/timer.rs`)

The lifecycle methods manage `total_interval_ms` and `accumulated_ms`:

- **`start()`** — initialises `total_interval_ms` to the first interval
  and `accumulated_ms` to `0`.
- **`pause()`** — adds the elapsed time of the current run segment to
  `accumulated_ms`.
- **`resume()`** — no change to `accumulated_ms`; the UI reads it to
  compute the negative `animation-delay`.
- **`on_interval_fire()`** — resets both fields for the new 5-minute
  interval (`total_interval_ms = TIMER_INTERVAL`, `accumulated_ms = 0`).
- **`stop()` / `stop_all*()`** — no special progress cleanup needed;
  removing the entry or setting `Stopped` is enough for the UI to hide
  the bar.

## 4. Update the UI (`src/components/cell_popup.rs`)

Wrap the hours `<input>` in a `<div class="popup-hours-container">`.
Inside that container, conditionally render the progress bar `<div>`
based on `timer_mgr.progress_info()`. This applies to both existing
entry rows and dynamic new entry rows.

```rust src/components/cell_popup.rs
    <div class="popup-hours-container">
        <input
            type="text"
            class="popup-hours"
            // ... existing props ...
        />
        {move || {
            let tid = tid_for_progress.clone();
            if let Some(ProgressInfo { duration_ms, offset_ms, running }) = timer_mgr.progress_info(&tid) {
                let cls = if running {
                    "timer-progress-bar timer-progress-running"
                } else {
                    "timer-progress-bar timer-progress-paused"
                };
                view! {
                    <div
                        class=cls
                        style=format!("--timer-duration: {}ms; --timer-offset: -{}ms", duration_ms, offset_ms)
                    ></div>
                }.into_any()
            } else {
                view! { <span class="timer-progress-spacer"></span> }.into_any()
            }
        }}
    </div>
```

## 5. CSS (`style/timesheet.css`)

The progress bar is positioned absolutely at the bottom of the hours
input. A `@keyframes` animation grows the width from 0 % to 100 %.
The duration and starting offset are injected via CSS custom properties.

```css style/timesheet.css
.popup-hours-container {
	position: relative;
	display: inline-block;
	width: 80px;
	flex-shrink: 0;
}

.popup-hours-container .popup-hours {
	width: 100%;
	box-sizing: border-box;
}

.timer-progress-bar {
	position: absolute;
	bottom: 0;
	left: 0;
	height: 2px;
	width: 0%;
	background: linear-gradient(to right, #0052cc, #4c9aff);
	border-radius: 0 0 2px 2px;
	z-index: 10;
	pointer-events: none;
}

.timer-progress-running {
	animation: timer-progress var(--timer-duration, 300000ms) linear 1 forwards;
	animation-delay: var(--timer-offset, 0ms);
	animation-play-state: running;
}

.timer-progress-paused {
	animation: timer-progress var(--timer-duration, 300000ms) linear 1 forwards;
	animation-delay: var(--timer-offset, 0ms);
	animation-play-state: paused;
}

@keyframes timer-progress {
	from {
		width: 0%;
	}
	to {
		width: 100%;
	}
}

.timer-progress-spacer {
	display: none;
}
```

Key points:

- Uses standard `animation` / `@keyframes` — no `-webkit-` prefixes.
- `animation-play-state: paused` freezes the bar exactly where it is
  when the timer is paused; no JavaScript is needed to hold the position.
- The negative `animation-delay` (via `--timer-offset`) makes the
  animation start partway through on resume, so the bar picks up from
  the correct position.
- `animation-fill-mode: forwards` (part of the shorthand) keeps the bar
  at 100 % width when the animation completes; it resets to 0 % when
  `on_interval_fire` updates the entry and the UI re-renders.
