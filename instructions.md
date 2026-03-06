# Timesheet

## Goal

Provide a tool to register and maintain a log of worked hours on Upland Jira work items.

## Architecture

Source code in Rust, using the Leptos framework for a full stack, server-side rendered web application, based on Actix web.

### idiomatic Rust

    - Avoid unwrap(), expect(), and panic(); always handle Option and Result using combinators like map, and_then, or pattern matching.
    - Prevent mut variables: try to use helper functions instead.
    - Prefer constructing HashMaps using `Iterator::collect` from a collection of (key, value) tuples, rather than creating a mutable map and inserting items one by one.
    - When a value (such as a String or Vec) is needed both in a closure (e.g., move || { ... }) and in the outer scope, clone it before the closure to avoid move errors and ensure idiomatic, clear ownership handling.
    - If a value is needed multiple times inside an Fn or FnMut closure (such as in event handlers or Leptos view! macros), clone it inside the closure before use, since these closures may be called more than once and cannot move non-Copy values.
    - When handling auxiliary data (such as user profile, avatar, or other metadata) returned alongside main data from the server, store it in a dedicated signal (e.g., `RwSignal<Option<(String, String)>>`) at the top of your component, and update it in your data-loading effect. Do not embed such auxiliary data inside unrelated structs like TimesheetData.
    - Always access auxiliary signals directly for display or logic, and use Option/Result combinators (map, and_then, unwrap_or, etc.) to safely extract inner values for UI, avoiding panics and ensuring robust updates.
    - Keep main state (e.g., timesheet data) and auxiliary state (e.g., user profile) clearly separated in your component logic and signals.
    - When auxiliary data (such as user profile) is not available on first load, do not show a placeholder or spinner; simply leave the field blank and update it only when the data is present in the cache or response.
    - Ensure cache warm-up on the server includes auxiliary data, so subsequent client loads can display it immediately if available.

### PopupInfo and Git Log Cell Behavior

- The `PopupInfo` struct includes a boolean flag `is_git_log` to indicate whether the popup is for a git log cell.
- When a timesheet cell is clicked, `is_git_log` is set to `true` if the cell contains git commit messages but no worklog entries (i.e., it is a "git log cell").
- For all other cells (with worklogs or empty), `is_git_log` is set to `false`.
- This flag can be used to control whether the contents of the cell's title attribute should be added to the last (blank) worklog comment, or to trigger other git-log-specific UI or logic.

### Valid HTML and CSS

- All `<img>` elements must include a valid `alt` attribute describing the image, to ensure accessibility and HTML5 validity.

### Leptos closure and signal usage guidelines

- When using signals (e.g., `RwSignal`, `Signal`) in Leptos, always access them inside a closure (`move || { ... }`) when used in `view!` or reactive contexts.
- If you need to use a variable (such as a list or struct) in multiple closures, wrap it in `Arc` (not `Rc`) and clone the reference for each closure. This prevents move errors and ensures thread safety.
- Avoid using `move` on the outer closure if you need to use the same variable in multiple inner closures. Instead, clone the variable for each closure.
- Prefer `Arc` for shared data in Leptos components, as Leptos requires values used in reactive closures to be `Send`.
- Example pattern:

```rust
use std::sync::Arc;
let items = Arc::new(vec![...]);
let closure1 = { let items = items.clone(); move || { /* use items */ }};
let closure2 = { let items = items.clone(); move || { /* use items */ }};
```

- If you encounter errors about values being moved or not implementing `Send`, review your closure captures and use `Arc` as needed.
- When capturing an `Option<HashMap<...>>` or similar non-`Copy` data (such as from a signal or struct) for use in closures (e.g., event handlers or `view!` macros), always clone the value before the closure and use the clone inside. This avoids borrow checker errors about values escaping the closure or being moved multiple times. Example:

```rust
let git_commits = ts.git_commits.clone();
let git_commits_for_closure = git_commits.clone();
on:click=move |_| {
    let is_git_log = entries2.is_empty() && git_commits_for_closure.as_ref().and_then(|map| map.get(&format!("{}:{}", ck2, cell_date))).is_some();
    // ...
}
```

- Valid HTML5 markup: the rendered HTML should pass the W3C validator
- Valid CSS3 styles, using CSS variables and media queries, with a focus on accessibility and responsiveness, and ensuring cross-browser compatibility, and using CSS Grid and Flexbox for layout, and using CSS animations and transitions for smooth user interactions
- never use inline styles

### External servers

    - Upland Jira: https://uplandsoftware.atlassian.net/rest/api/latest/
    - Bitbucket: https://api.bitbucket.org/2.0/ (DISABLED: Bitbucket API integration has been removed due to API deprecation)

### Web server

#### Track user settings

    - Upland Jira
    	- email address of user
    	- API token

    - Git
    	- folder of Git workspace to track
      - poll interval for new commits
    - Preferences
    	- average hours per week
    	- average hours per day

    - Bitbucket integration is currently disabled due to API deprecation.

#### Internationalisation

    - No literal strings should be used inside Leptos' view! blocks, but translated strings based on the locale reported by the browser
    - Numbers with a decimal point should also use a separator as specified by the browser's Locale

#### Caching

    - JQL queries to Upland Jira may take several seconds to respond, so we best cache these results for subsequent use

### Front end (web app)

#### Display of logged hours

    - short: Logged hours for a work item are displayed as a decimal number. The number should be formatted depending on the number of digits: a total >= 100: only the rounded integer value, 100 < total <=10: rounded to one decimal and for totals < 10: rounded to two decimals. (The decimal separator is determined on the server).
    - long:  Logged hours for a work item are displayed as a sequence e.g., 1w 3d 5h 25m, where 'w' stands for weeks, 'd' for days, 'h' for hours and 'm' for minutes. These values are calculated from the number of logged hours, where the number of days and weeks depend on the user settings on the server. So, when 70.25 hours are logged and the 'hours per week' are set to 36 and the 'hours per day' are set to 9, this will be displayed as '1w 3d 3h 15m'

#### Timesheet view

This is the main view of the application. Here a overview of hours spent per work item is presented as a grid. The rows are the work items the user has worked on in the selected period. The work items can be found on Upland Jira.

##### Accessing and using the Settings dialog

- The settings dialog can be accessed at any time by clicking the gear icon button located at the right side of the bottom navigation bar. This button has only an icon and a title attribute ("Open settings") for accessibility.
- When the settings dialog is open, you can update your credentials and preferences. The dialog now includes both an **OK** button (to save changes) and a **Cancel** button (to close the dialog without saving).
- Pressing **Cancel** will discard any changes and return you to the timesheet view.
- The Bitbucket credentials fields are visible but disabled, with a note explaining that Bitbucket integration is currently disabled due to API deprecation. You may still store credentials, but they are not used.
- The refresh button is also located at the right of the navigation bar, next to the settings button, and is icon-only (no label), with a title attribute ("Refresh cached work items").

(Bitbucket work item integration is disabled due to API deprecation.)

Each work item is presented as a row header, with the Jira icon for the work item type, the reference key (e.g., SHARED-12345 or TIM-104) and it's summary. The row header also includes the year-to-date total of hours logged by the current user for that work item.

In addition to work items with work logs in the selected period, the timesheet automatically includes all Jira tickets assigned to the user with status "In Progress" or "Code Review". Both result sets are merged and deduplicated by issue key. Work items are sorted by key only (natural order).

Depending on the available viewport width, one or more column groups will be shown. Each column group consists of five columns for the working days (monday through friday) and one for the weekend. The working days have their abreviated name in the header, as well as the date (day and month). Each column header includes also the total number of logged hours for that day. The group also has a total of logged hours for that week.

There should also be the possibility to go back or forward one week, or enter a specific date. Navigating means that the rightmost column group shows the selected period. If that period is no longer the current week (the week that includes today), there is also a Today button to return to the default period.

Each cell displays the total number of logged hours for the associated work item and date in short format. If there are more than one work log entries for the cell, it's title attribute contains all the descriptions with their logged work hours in long format. If there is only one work log, then the title attribute only holds the description.

Zero values will not be displayed (blank cell)

When the user clicks on a cell that corresponds to a work item and date combination, then a popuup will be shown over that cell. in it are all the individual log entries, with the logged hours in long format, plus one extra row for a new entry. Existing entries may be modified, or deleted and an new item will be added when both a number of work hours and a description are given. The work hours may be entered in either short or long format.

#### Work item search

The column header for the work-item column contains a search box. As the user types, the app queries Jira's issue picker endpoint for work items whose key or summary matches the search text (fuzzy / partial matching — e.g. typing "96320" finds SHARED-96320). Results are shown in a dropdown list (max 12 items) displaying the issue-type icon, key, and summary. Selecting an item adds it to the top of the timesheet grid. Typing is debounced (300 ms) and earlier in-flight requests are logically cancelled via a version counter so that only the most recent response is applied. The search box is cleared when the user navigates to a different week.

---

## Implementation decisions

#### Cell popup default comment logic

- The logic for pre-filling the new work log comment in the cell popup distinguishes between normal Jira worklog cells and Git log cells.
- Each cell in the timesheet grid includes a `data-git` attribute (set to `"true"` for Git log cells).
- When opening the cell popup, an `is_git_log` flag is passed to the popup logic.
- The new comment input is only prefilled with the rendered comment if `is_git_log` is true (i.e., for Git log cells).
- For normal Jira worklog cells, the new comment input is left empty by default.

### Leptos version

Using Leptos 0.8.x with nightly Rust. SSR via Actix-web with WASM hydration on the client.

### Project structure

### I18n

- The server must never output literal strings, but always use translated ones based on the selected language.
- **All user-facing strings** (including labels, placeholders, alt/title attributes, error messages, etc.) must use translation keys and the i18n system, never hardcoded literals, including in SSR output and all HTML attributes.
- `I18n` struct stored as `RwSignal<I18n>` in Leptos context.
- Browser locale detected via `navigator.language()` on hydration; defaults to "en" during SSR.
- Decimal separator derived from language code (comma for FR/DE/NL/ES/IT/etc., dot for EN).
- All view text and attributes use `i18n.get().t(keys::CONSTANT)` — no literal strings in `view!` blocks or attribute values.
- Supported languages: English (complete), French (complete). Easy to extend.

### Hour formatting

- **Short format**: variable precision based on magnitude (≥100→int, ≥10→1dp, <10→2dp)
- **Long format**: `Xw Yd Zh Wm` derived from user's hours_per_day and hours_per_week settings
- **Parsing**: accepts both decimal ("2.5") and long ("2h 30m") input in the cell popup. The `parse_hours` function accepts four locale-specific unit label parameters (`w_label`, `d_label`, `h_label`, `m_label`) so that e.g. Dutch "1u 5m" is recognised alongside English "1h 5m". A locale-aware regex is built at parse time from the supplied labels; English labels (`w`/`d`/`h`/`m`) are always tried as a fallback so input in the default format is never rejected regardless of the active language.
- **Note**: The spec example states 70.25h at 36h/week 9h/day = "1w 3d 3h 15m", but correct calculation yields "1w 3d 7h 15m" (36+27+7+0.25=70.25). Implementation uses the correct algorithm.

### API integration

- **Jira**: Basic auth (email:token), uses v3 API (`/rest/api/3`). Issue search via the new `/search/jql` endpoint (the old `/search` was removed — see [CHANGE-2046](https://developer.atlassian.com/changelog/#CHANGE-2046)) with cursor-based pagination (`nextPageToken`/`isLast`). Work-item search uses the `/issue/picker` endpoint for fuzzy matching on both key fragments and summary text, followed by a bulk `key in (...)` JQL fetch to retrieve full issue-type icons and metadata. Per-issue worklog fetch filtered by author+date. CRUD via POST/PUT/DELETE on `/issue/{key}/worklog` with comments in ADF (Atlassian Document Format) as required by v3.
- **Bitbucket**: (DISABLED) Bitbucket API integration has been removed due to API deprecation.
- **Caching**: Three-level in-memory cache with 5-day TTL and automatic expiry:
   1. **JQL search cache** (`jira_search:{jql}`) — work item keys per search query, avoids repeated issue searches.
   2. **Per-issue worklog cache** (`jira_worklogs:{issue_key}`) — stores ALL worklogs by the active user for an issue (regardless of date range). Navigating to a different week filters from this cache instead of hitting Jira again.
   3. **Assembled TimesheetData cache** (`timesheet_data:{start}:{end}`) — the fully assembled response for a date range. Revisiting the same week is instantaneous.
   - Cache invalidation: when a worklog is added, updated or deleted, the per-issue worklog entry and all assembled TimesheetData entries are evicted so the next request fetches fresh data.
   - Prefetch on startup: a background task warms the cache for the current week, the previous week, and (when it is not in the future) the next week, so the very first page load is served from cache.
   - Prefetch on navigation: every time `get_timesheet_data` serves a week (whether from cache or freshly fetched), it spawns a background task that prefetches the previous week and, when its Monday is not after today, the next week. Weeks that are already cached are skipped. This means that navigating one week backward or forward is almost always an instant cache hit.

### Week navigation UX

When the user navigates between weeks the previous grid remains visible with a semi-transparent loading overlay on top, instead of being replaced by a blank "Loading…" message. This is achieved by storing the last successfully loaded `TimesheetData` in a `RwSignal` and rendering from that signal at all times. The async `Resource` updates the signal when new data arrives; a separate `is_loading` signal controls the overlay.

### Weekend handling

Saturday and Sunday worklogs combined into a single "W/E" column per the spec.

### Multi-week display

The timesheet grid dynamically renders one or more week column groups based on the browser viewport width:

- **≤ 1000 px** → 1 week (single column group, original behaviour)
- **1001–1300 px** → 2 weeks
- **every additional 300 px** → +1 week (e.g. 1601 px → 4 weeks)

The rightmost column group always corresponds to the selected (navigated-to) week; earlier weeks extend to the left. Each column group contains five weekday columns, a combined weekend column, and a week-total column. When multiple groups are visible, the total column header shows the ISO week number (e.g. "W23" in English, "S23" in French) instead of the generic "Total" label.

**Implementation details:**

- A `compute_num_weeks(viewport_width)` helper derives the group count from the viewport width using integer arithmetic: `1 + (w − 701) / 300` when `w > 1000`, else `1`.
- A `num_weeks` reactive signal is initialised from `window.innerWidth` at hydration and kept up-to-date via a raw `resize` event listener (`wasm_bindgen::closure::Closure` + `addEventListener`). The signal only fires when the computed count actually changes, avoiding unnecessary re-renders during continuous resize.
- The `Resource` source tuple includes both `selected_monday` and `num_weeks`. When the viewport crosses a breakpoint the date range widens/shrinks and a refetch is triggered. Per-issue worklog caches (which are date-independent) ensure that expanding the range is fast.
- Column groups are visually separated by a 3 px left border (`.week-separator` CSS class) on the first day column of each non-first group.
- During SSR, `num_weeks` defaults to `1`; the client-side hydration sets the true value immediately.
- A manual "Refresh" button in the bottom navigation bar clears the entire server-side cache (`cache::clear_all()`) and triggers a refetch, allowing the user to force fresh data from Jira.

### Work item search

- Uses Jira's `/rest/api/3/issue/picker` endpoint for fuzzy matching on both issue keys and summary text.
- The picker returns candidate keys across multiple sections (history, current search); results are deduplicated and capped at 12.
- A follow-up bulk JQL fetch (`key in (...)`) enriches each result with the correct issue-type icon URL and type name, since the picker only returns relative icon paths.
- On the client, each keystroke bumps a monotonically increasing version counter. An async task waits 300 ms (debounce via a `js_sys::Promise`-based sleep), then checks whether its version is still current before firing the server call. Stale responses are discarded by the same version check, effectively cancelling all earlier in-flight requests.
- The dropdown is rendered outside the `<table>` (at the `<div class="timesheet">` level) with `position: fixed`, so it is never clipped by the table's `overflow` or sticky headers. Its position is computed from the search input's bounding rect.
- Selecting an item inserts it at position 0 in the work-items list so it appears as the first grid row. Duplicates are prevented by key check.
- The search state (query, results, dropdown, version counter) is cleared on week navigation.

### Work log entry

1. Save button enablement logic:
   - Implement a validation function that checks all current entry signals (excluding the extra row if blank).
   - Use a derived signal or computed property to enable/disable the Save button.

2. Keyboard shortcuts:
   - Add a keydown event listener to the popup container.
   - On Enter: If Save is enabled, trigger the save handler.
   - On Esc: Trigger the cancel handler.

3. Dynamic extra row management:
   - Track all entry rows (existing + new) in a signal/Vec.
   - On blur of the extra row’s duration input, if the value is valid and non-blank, append a new blank row.
   - Ensure only one extra row is blank at any time.

4. State tracking for created/modified/deleted entries:
   - Maintain three collections: Created, Modified, Deleted.
   - On Save, batch process these collections with the appropriate server calls.

5. Validation logic:
   - Use or extend `parse_hours` for duration validation.
   - For comments, check for non-empty strings as required.

6. UI feedback:
   - Optionally, show inline error messages or input highlighting for invalid fields.
   - Bind Save button’s `disabled` property to the validation signal.

7. Other minor enhancements:
   - Ensure the suggested comment from git log is always copied into the last available comment input when appropriate.
   - Make sure the popup closes on Save or Cancel.

### ADF comment rendering

- **Full ADF → HTML conversion** (`adf_to_html`): recursive renderer covering all common Atlassian Document Format node types:
   - Block nodes: `paragraph`, `heading` (h1–h6), `bulletList`, `orderedList`, `listItem`, `codeBlock` (with language class), `blockquote`, `rule`, `table`/`tableRow`/`tableHeader`/`tableCell` (with colspan/rowspan), `panel` (info/note/success/warning/error), `expand` (→ `<details>`), `date` (epoch → formatted), `mediaSingle`/`media` (placeholder).
   - Inline nodes: `text`, `hardBreak`, `mention`, `emoji`, `inlineCard`, `status`.
   - Text marks: `strong`, `em`, `underline`, `strike`, `code`, `link` (with title), `textColor`, `subsup`.
- **Structured plain-text extraction** (`adf_extract_text`): produces newline-separated paragraphs, `•` bullet markers, `>` blockquote prefixes, `---` for rules, and proper mention/emoji/status rendering. Used for cell tooltips (`title` attribute) and as the fallback edit text.
- **Trivial-comment suppression and improved plain-text detection**: when the ADF contains only a single paragraph with just text children, the comment is treated as plain text (concatenation of those texts), and `comment_html` is left empty so the popup shows a simple text input instead of a redundant preview.
- **ADF round-tripping**: the raw ADF JSON is stored on `WorklogEntry.comment_adf`. When a worklog has a rich ADF preview, the popup shows it read-only (no text input) with a 🔗 link to the worklog in Jira. On save, the original ADF is sent back unchanged, preserving all formatting. For plain-text entries, the text input is shown as before; if the user didn't edit it, the original ADF is still preserved.
- **CSS**: comprehensive styles for `.adf-preview` (inline flex layout, accent border, scrollable, max-height constrained) and all rendered elements — headings, lists, dark-themed code blocks, blockquotes, tables, links, panels (5 colour variants matching Jira's palette), status lozenges (6 colours), mentions (blue badge), expandable sections, date badges, and media placeholders.

### Dependencies added

- `reqwest` 0.12 (SSR-only) — HTTP client for Jira APIs

- `base64` 0.22 (SSR-only) — Basic auth header encoding

- `regex` 1.x — Jira key extraction from PR branches, long-format hour parsing

- `log` 0.4 — Structured logging

- `anyhow` 1.x — Error handling for Git integration
- `js-sys` 0.3 (hydrate-only) — Promise-based debounce timer for work-item search

- `wasm-bindgen-futures` 0.4 (hydrate-only) — JsFuture for awaiting the debounce Promise

- `send_wrapper` 0.6 — Wraps `!Send` closure-based state so it can live in Leptos context (already a transitive dep of leptos)

### Git commit integration

- **Git commit analysis from configured workspace folder** is now implemented.

- The app scans the configured Git workspace for commits whose messages start with a work item key (e.g., `SHARED-12345`), matching only keys present in the timesheet grid.
- For each cell (work item, date) with no worklog but with one or more Git commits, a question mark (`?`) is shown. The cell's `title` attribute contains all commit messages for that cell, concatenated with newlines.
- Commit analysis is cached per date range and workspace folder.
- All Git logic is in `src/api/git.rs` for consistency.
- The `anyhow` crate is required for error handling.
- **Cell popup integration:** When a cell represents Git log entries, the `data-git` attribute is set and the popup receives `is_git_log: true`. Only in this case is the default comment for a new worklog prefilled with the rendered commit messages. For all other cells, the new comment input is left empty.
- **Polling for new commits:** The browser schedules a recurring check for new git commits, with the interval (in minutes) configurable in the Git section of the settings dialog (`git_poll_interval_minutes`, default 5). After each interval, if neither the cell popup nor the settings dialog is open, a server function is called to check for new commits. If new commits are found, each work item key not already present in the displayed list of work items is automatically added as a new row in the timesheet view, using the commit message as the summary (until real metadata is fetched).

### Time tracking

- **Per-row timers** allow the user to start, pause, resume, and stop a timer on any worklog entry row inside the cell popup, but only when the popup's date column corresponds to today (including weekends). While a timer is active (running or paused) for a row, the duration input for that row is disabled so the user cannot overwrite the timer-managed value.
- **Single active timer rule:** only one timer can be running at a time. Starting a new timer automatically pauses any previously running timer.
- **Timer interval logic:**
   - On start, the current duration value is parsed and a browser `setTimeout` is scheduled for 2.5 minutes.
   - When that timeout fires, the duration is incremented by 5 minutes and a new 5-minute timeout is scheduled. This repeats until stopped.
   - On pause, the remaining time of the current interval is recorded. On resume, a timeout is scheduled for that remaining time; once it fires the 5-minute cycle continues.
   - On stop, the duration is **not** updated (the previous interval already accounted for it).
- **Timer state management** lives in `src/components/timer.rs`. A `TimerManager` singleton is stored in Leptos context and wraps a `RwSignal<HashMap<TimerId, TimerEntry>>` so all timer phase queries are reactive. `TimerId` is `(issue_key, date, row_index)`.
- **Multiple simultaneous popups:** the cell popup system was refactored from a single `Option<PopupInfo>` to a `Vec<PopupInfo>` so that multiple popups can be open at the same time, each independently closeable.
- **Draggable popups:** each popup has a drag handle (hamburger icon ☰) rendered above the header. Mouse-down on the handle initiates a drag; `mousemove` / `mouseup` listeners on the window update the popup's `position:fixed` inline style via an `RwSignal<String>`. Drag state (`is_dragging`, start coordinates) is stored in `Rc<Cell<>>` rather than `RwSignal` because the window-level closures outlive the Leptos reactive scope and would panic on disposed signals. An `Arc<AtomicBool>` "alive" flag is shared between the forgotten window listeners and the close callback; when the popup closes the flag is set to `false` and the listeners become harmless no-ops, avoiding `unreachable` panics from accessing disposed `RwSignal`s.
- **Closing a popup stops its timers:** the Close and Escape handlers call `timer_mgr.stop_all_for_popup()` to cancel all running and paused timers belonging to that popup before removing it.
- **I18n:** four new translation keys (`timer_start`, `timer_pause`, `timer_resume`, `timer_stop`) were added in English, French, and Dutch for the timer button tooltips.
- **CSS:** new style rules for `.timer-controls`, `.timer-btn`, `.timer-play-pause`, `.timer-stop`, and `.popup-draggable-title` provide compact, colour-coded timer buttons and the draggable title bar.

### Popup flush-on-navigate

- **Problem:** when a user has unsaved edits in an open cell popup and triggers a navigation action (week change, cache refresh, language switch, or browser leave/refresh), those edits are silently discarded.
- **Solution:** a `PopupDraftManager` coordinator (`src/components/popup_flush.rs`) that auto-saves dirty-and-valid popups before any navigation-like action proceeds.

#### PopupDraftManager design

- Stored in Leptos context (provided by `TimesheetView` via `provide_popup_flush_context()`).
- Each open `CellPopup` registers itself on mount with three closures:
   - `is_valid` — delegates to the existing `save_enabled` memo (all hours parse, comments non-empty when required).
   - `is_dirty` — returns `true` when any existing entry's hours or comment changed from its initial value, any entry is marked for deletion, or any new entry row has non-empty hours. An `initial_hours` field was added to `ExistingEntry` for this comparison.
   - `save_fn(latch: Option<FlushLatch>)` — calls the same `on_save` closure wired to the Save button. The `on_save` closure accepts an optional `FlushLatch`; when present, `latch.arrive()` is called inside the `spawn_local` async block **after** all server calls complete. When `None` (normal Save-button / keyboard path), the save behaves as fire-and-forget.
- Popups are automatically unregistered when closed: the `on_close` callback wrapper calls `flush_mgr.unregister(popup_id)`.
- Two flush methods:
   - `flush_all()` — fire-and-forget. Iterates registered popups, saves those that are both dirty and valid, silently skips invalid ones. Used by week navigation, cache refresh, settings, and `beforeunload`.
   - `flush_all_then(on_complete)` — same selection logic, but creates a `FlushLatch` (shared countdown) that fires `on_complete` only after every triggered save's async server work has finished. If no popups need saving the callback fires immediately. Used by the language-change handler to defer `window.location().reload()` until the server state is up-to-date.
- A `flushing` re-entrancy guard prevents duplicate saves from rapid concurrent triggers. If the guard blocks, `flush_all_then`'s callback still fires immediately so callers are never left hanging.
- Inner state uses `Rc<dyn Fn(Option<FlushLatch>)>` closures (non-Send), wrapped in `send_wrapper::SendWrapper` so the type satisfies Leptos context's `Send + Sync` bounds while remaining single-thread-only at runtime.

#### FlushLatch

- A simple `Rc<RefCell<…>>` countdown latch. Created with a `count` (number of dirty-and-valid popups) and an `on_complete` callback.
- Each save's `spawn_local` task calls `latch.arrive()` after its server calls finish; when the last arrival decrements the counter to zero, `on_complete` fires exactly once.
- If `count == 0` at construction time, the callback fires immediately inside `FlushLatch::new`.
- This avoids the race condition where a page reload (e.g. language switch) would kill in-flight `fetch` requests before the server received the updated worklog, causing the reloaded page to serve stale cached data.

#### Integration points

All callers that trigger a data fetch or page reload use `flush_all_then` so the action only proceeds once every in-flight save has reached the server and invalidated the assembled-data cache. Callers that don't refetch data use fire-and-forget `flush_all`.

- **Week navigation** (`src/components/week_navigator.rs`): `flush_all_then` defers the `selected_monday.set(…)` call — and therefore the `Resource` refetch — until all saves have completed on the server. Applies to all four handlers: previous week, next week, today, and date input change.
- **Cache refresh** (`on_refresh` in `TimesheetView`): `flush_all_then` defers the `clear_cache()` + `data.refetch()` spawn so the server-side cache is only wiped and re-fetched after the worklog updates have landed.
- **Language change** (`on_lang_change`): `flush_all_then(callback)` where the callback performs `lang_signal.set`, `i18n.set`, and `window.location().reload()`. The localStorage write happens eagerly (before flush) so it is never lost, but the actual reload is deferred until every in-flight save has reached the server and invalidated the assembled-data cache. This prevents the reloaded page from serving stale worklog totals.
- **Settings open** (`on_open_settings`): `flush_all()` (fire-and-forget) before showing the settings dialog. No data fetch follows, so ordering is not critical.
- **Browser leave / refresh** (hydrate-only): a `beforeunload` event listener calls `flush_all()` (fire-and-forget, best-effort) when popups are open and invokes `ev.prevent_default()` to trigger the browser's leave-page confirmation dialog.

#### Behavior decisions

- Only dirty **and** valid popups are saved. Invalid popups are left open (the user sees validation state when they return).
- Clean (unchanged) popups are silently closed by the save-triggered `on_close` path during flush — no server calls are made for them since `is_dirty` returns false and they are skipped.
- The `save_fn` reuses the exact same code path as the Save button, including stopping timers and spawning async server calls.
- **Async-completion race:** because `on_save` closes the popup synchronously and spawns server calls via `spawn_local`, a caller that proceeds immediately (e.g. `window.location().reload()` or `selected_monday.set()`) can race with the in-flight requests — the data refetch may reach the server before the worklog update does, returning stale cached data. All callers that trigger a data fetch or page reload therefore use `flush_all_then` to defer their action until the latch signals completion. Only `beforeunload` and settings-open use fire-and-forget `flush_all` since they don't refetch data.

### Future work

- multi user support
- OAuth2/SSO
- CSV/Excel export
- Offline worklog queue for intermittent connectivity
- Dark mode support
- Keyboard navigation in timesheet grid
