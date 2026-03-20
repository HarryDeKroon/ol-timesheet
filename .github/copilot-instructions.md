# Copilot Instructions — OL Timesheet

## What this project does

Full-stack Rust web app for tracking work hours (worklogs) against Upland Jira tickets. Features a reactive timesheet grid, real-time timers, Git commit integration, multi-language support (EN/FR/NL), and server-side caching.

## Build & run

Requires [`cargo-leptos`](https://github.com/leptos-rs/cargo-leptos):

```bash
cargo install cargo-leptos
cargo leptos serve        # dev server at http://localhost:8081
cargo leptos build        # production build
```

There are currently no automated tests. The `cargo test` command does not apply.

## Architecture

### Stack

- **Server**: Axum + Tokio (`ssr` feature) — serves SSR HTML and Leptos server functions at `/api`
- **Client**: WASM + Leptos hydration (`hydrate` feature)
- **CSS**: single file `style/timesheet.css` — CSS Grid/Flexbox, CSS variables, no inline styles

### Feature flags

All code is gated on two mutually exclusive Cargo features:

- `ssr` — server-only code (Axum routes, Jira/Git API calls, disk I/O, in-memory cache)
- `hydrate` — WASM/browser-only code (WebSocket, `web-sys`, `wasm-bindgen`, timers)

Use `cfg_if::cfg_if!` blocks or `#[cfg(feature = "ssr")]` / `#[cfg(feature = "hydrate")]` attributes consistently. Never put browser APIs behind `ssr` or vice versa.

### Module map

| Module | Responsibility |
|---|---|
| `src/model.rs` | Shared domain types: `Settings`, `WorkItem`, `WorklogEntry`, `TimesheetData`, `ConnectionStatus` |
| `src/app.rs` | Root `App` component; auth check; routes to `SettingsDialog` or `TimesheetView` |
| `src/components/` | All Leptos UI components |
| `src/api/jira.rs` | Jira REST v3 client (fetch/create/update/delete worklogs, issue search, caching) |
| `src/api/git.rs` | Git workspace commit scanning (SSR + client polling) |
| `src/api/cache.rs` | SSR-only in-memory cache, 5-minute TTL |
| `src/connection.rs` | `ConnectionState` Leptos context; WebSocket heartbeat with exponential back-off reconnect |
| `src/formatting.rs` | `format_hours_short`, `format_hours_long`, `parse_hours` — locale-aware |
| `src/i18n.rs` | `I18n` struct and all translation keys (constants in `i18n::keys`) |
| `src/flags.rs` | SVG flag icons as embedded constants |

### Server functions

Leptos server functions bridge SSR ↔ client. Define with `#[server(FnName, "/api")]`. They compile to a POST handler on the server and a `fetch` call in WASM. Place feature-gated implementation logic inside `#[cfg(feature = "ssr")]` blocks within the same file where possible.

### Caching (SSR only)

Three-level cache in `src/api/cache.rs` (global `Mutex<HashMap>`):

1. JQL search results per query
2. Per-issue worklogs (date-independent; navigation filters from cache)
3. Fully assembled `TimesheetData` per date range

On worklog write/delete, evict the affected per-issue entry and all `TimesheetData` entries. A background task warms the current, previous, and next week on startup and after every navigation.

### Settings persistence

Settings are stored as `settings.json` in the OS config directory (`directories::ProjectDirs`). A UUID token file is used for session auth (stored in `localStorage` on the client).

### External APIs

- **Upland Jira**: `https://uplandsoftware.atlassian.net/rest/api/3/` — Basic auth (`email:token`), issue search via `/search/jql` (cursor-based), work-item fuzzy search via `/issue/picker`, worklogs via `/issue/{key}/worklog`
- **Bitbucket**: disabled (API deprecated) — fields kept in `Settings` for backward compatibility but not used at runtime

### Jira API call patterns

All Jira HTTP calls are in `src/api/jira.rs` (SSR-only) using a process-global `LazyLock<reqwest::Client>`.

**Authentication** — every request sends `Authorization: Basic <base64(email:token)>`.

**Issue search** — uses the **new** `/rest/api/3/search/jql` endpoint (the old `/search` was removed; see CHANGE-2046). Pagination uses `nextPageToken`/`isLast` (cursor-based, not offset). The loop continues until `is_last == true` or `next_page_token` is absent. Always pass `fields=summary,issuetype` to limit payload size.

```
GET /rest/api/3/search/jql?jql=<encoded>&fields=summary,issuetype&maxResults=100
                            &nextPageToken=<token>   ← only on subsequent pages
```

`fetch_work_items` issues **two** JQL queries and deduplicates:
1. `worklogAuthor = "{email}" AND worklogDate >= "{start}" AND worklogDate <= "{end}"`
2. `assignee = "{email}" AND status IN ("Code Review", "In Progress")`

**Work-item fuzzy search** — two-step:
1. `GET /rest/api/3/issue/picker?query=<text>` — returns ranked candidate keys across `sections[]` (dedup across sections, cap at 12)
2. `GET /rest/api/3/search/jql?jql=key in ("K1","K2",...)` — bulk fetch to get icon URLs and type names (the picker only returns relative icon paths)

**Worklogs** — `GET /rest/api/3/issue/{key}/worklog` fetches **all** worklogs; author filtering (`email_address == settings.email`) and date filtering are applied in Rust, not in the API call. The full unfiltered list is stored in cache so week navigation never re-fetches.

**Worklog writes** — all comments must be in [Atlassian Document Format (ADF)](https://developer.atlassian.com/cloud/jira/platform/apis/document/structure/) as required by v3:
- `POST /rest/api/3/issue/{key}/worklog` — create; body includes `started` (`"{date}T12:00:00.000+0000"`), `timeSpentSeconds`, and `comment` as ADF
- `PUT /rest/api/3/issue/{key}/worklog/{id}` — update; when `original_adf` is available it is sent back unchanged (ADF round-trip), otherwise `make_adf_comment(text)` wraps plain text in a single-paragraph ADF doc
- `DELETE /rest/api/3/issue/{key}/worklog/{id}` — delete

After any write, call `invalidate_worklogs_for_issue(key)` which removes both the per-issue cache entry and **all** `timesheet_data:*` entries.

**User profile** — `GET /rest/api/3/myself` returns `JiraUserProfile` with `displayName` and `avatarUrls["48x48"]`. Returned alongside `TimesheetData` as `Option<(String, String)>` (avatar URL, display name); not cached.

**ADF comment handling**:
- `JiraComment` is `#[serde(untagged)]` — deserialises as `Plain(String)` or `Adf(serde_json::Value)`
- Single-paragraph ADF with only `text` children is treated as plain text (no HTML preview shown)
- Rich ADF is converted to HTML via `adf_to_html` (recursive) and to plain text via `adf_extract_text` (for cell tooltips)

## Key conventions

### Idiomatic Rust in this codebase

- **No `unwrap()`/`expect()`/`panic()`** — always handle `Option`/`Result` with combinators or pattern matching
- **No `mut` variables** where a helper function suffices
- Build `HashMap`s from `Iterator::collect` on `(key, value)` tuples, not by inserting into a mutable map
- Use `Arc` (not `Rc`) for shared data captured by multiple Leptos closures — Leptos requires `Send`
- Clone values **before** a `move` closure when also needed in the outer scope; clone **inside** `Fn`/`FnMut` closures when called multiple times

### Leptos signal / closure pattern

```rust
use std::sync::Arc;
let items = Arc::new(vec![...]);
let items_for_click = items.clone();
let on_click = move |_| { /* use items_for_click */ };
let items_for_render = items.clone();
view! { <div>{move || items_for_render.len()}</div> }
```

Always access signals inside a closure in `view!` — never call `.get()` at component construction time for reactive values.

### I18n — no literal strings in views

Every user-facing string must go through the i18n system. No hardcoded string literals in `view!` blocks, HTML attributes, or SSR output.

```rust
// ✅ correct
let i18n = use_context::<RwSignal<I18n>>().unwrap();
view! { <span>{move || i18n.get().t(keys::SAVE)}</span> }

// ❌ wrong
view! { <span>"Save"</span> }
```

Add new keys as `pub const` in `i18n::keys`, then add translations for all three languages (EN/FR/NL) in the `TRANSLATIONS` static.

### HTML/CSS rules

- All `<img>` elements must have a valid `alt` attribute (W3C validity)
- No inline `style="..."` attributes — use CSS classes and variables
- CSS must be valid CSS3, using CSS variables for theming, Grid/Flexbox for layout

### Separate auxiliary state from main state

Keep `TimesheetData` (worklogs, work items) separate from auxiliary signals like user profile `RwSignal<Option<(String, String)>>`. Do not embed unrelated data inside domain structs.

### Timer logic (`src/components/timer.rs`)

`TimerManager` is a Leptos context (call `provide_timer_context()` once in `TimesheetView`). It holds a `RwSignal<HashMap<TimerId, TimerEntry>>`. Timers are only available when `PopupInfo.is_today == true`.

**`TimerId`** identifies a row: `{ issue_key, date, row_index }`.

**`TimerPhase`** state machine:
- `Running` → `Paused { remaining_ms }` (pause) or `Stopped` (stop)
- `Paused` → `Running` (resume) or `Stopped` (stop)
- `Stopped` is terminal

**Interval timing** (browser `setTimeout`, `hydrate`-only):
1. **Start** — first interval is **2.5 min** (`TIMER_INTERVAL_SHORT = 150_000 ms`). Any currently `Running` timer is **paused** first (single-active-timer rule).
2. After 2.5 min fires — duration bumped by **5 min** (`TIMER_INTERVAL = 300_000 ms`); a new 5-min timeout is scheduled. Repeats indefinitely.
3. **Pause** — record `remaining_ms = interval_ms - elapsed` from `Performance.now()`. Duration is **not** updated.
4. **Resume** — schedule a timeout for `remaining_ms`; on fire, bump 5 min and continue with 5-min intervals.
5. **Stop** — cancel the timeout. Duration is **not** updated (the completed interval already applied it).

**Progress bar** — driven entirely by CSS animation (no JS polling). `TimerManager::progress_info()` returns `ProgressInfo` with:
- `is_first_interval` — selects animation duration (2.5 min vs 5 min)
- `offset_ms` — accumulated elapsed ms across pauses → used as negative `animation-delay`
- `running` — toggles `timer-progress-paused` CSS class
- `generation` — bumped on each new interval; used as a DOM key to force a fresh element and restart `@keyframes`

While a timer is `Running` or `Paused`, the duration input for that row is **disabled** (timer owns the value).

### Multi-week viewport breakpoints

`compute_num_weeks(viewport_width: f64) -> usize` (hydrate-only, in `timesheet_view.rs`):

```
w ≤ 1000 px  →  1 week
w > 1000 px  →  1 + (w - 701) / 300   (integer division)
```

Examples: 1001 px → 2, 1301 px → 3, 1601 px → 4, 1901 px → 5.

`num_weeks` is a `RwSignal<usize>` initialised from `window.innerWidth` at hydration. A raw `"resize"` event listener (via `wasm_bindgen::closure::Closure`, leaked with `.forget()`) updates it only when the computed count actually changes, avoiding unnecessary refetches.

The `Resource` source tuple is `(selected_monday, num_weeks)`. When the breakpoint is crossed the date range widens/shrinks and a refetch fires. The date range passed to `get_timesheet_data` spans `num_weeks` full weeks ending on `selected_monday + 6 days`.

**Column layout**: the rightmost group is always the selected week; earlier groups extend to the left. When `num_weeks > 1`, the "Total" column header shows the ISO week number (e.g. `W23` / `S23` in French) rather than the generic label. The first day column of each non-first group carries the `.week-separator` CSS class (3 px left border).

During SSR, `num_weeks` defaults to `1`; hydration sets the real value immediately on first paint.

### `PopupInfo` / Git log cells

`PopupInfo.is_git_log` is `true` when a cell has Git commits but no worklog entries. Only prefill the new comment input from commit messages when `is_git_log` is `true`; leave it empty for normal worklog cells.

### Connection state

Use `use_connection()` to access `ConnectionState` from context. Guard any user action that requires the server with `connection.is_available()`. Track in-flight API requests with `request_started()` / `request_finished()`.
