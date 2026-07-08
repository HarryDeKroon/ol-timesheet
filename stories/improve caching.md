# Improve caching and background refresh plan

## Goal

Reduce first-load and navigation latency by:
1. caching per week (Monday-based),
2. fetching Jira and Bitbucket activity in parallel,
3. incrementally refreshing current-week data in the background,
4. pushing delta updates to the browser over WebSocket.

This plan is implementation-ready for one autopilot pass.

## Verified baseline (current code)

- Timesheet request entrypoint: `src/components/timesheet_view.rs::get_timesheet_data`.
- Jira cache + prefetch logic: `src/api/jira.rs` (`fetch_work_items`, `fetch_worklogs`, `prefetch_adjacent_weeks`).
- Generic in-memory cache storage: `src/api/cache.rs`.
- Bitbucket activity fetch: `src/api/bitbucket.rs::fetch_timesheet_activity`.
- Existing WebSocket endpoint is heartbeat-only: `src/main.rs` route `/ws/heartbeat`.
- Cache invalidation on worklog writes already exists: `invalidate_worklogs_for_issue`.

## Definitions

- **week**: Monday..Sunday. Cache key uses the Monday date only.
- **period**: one or more contiguous weeks requested by UI.
- **cell key**: `<ISSUE_KEY>:<YYYY-MM-DD>`.

## Required behavior

### 1. Parallel source fetching

For every missing week, fetch from Jira and Bitbucket concurrently (Tokio tasks, not blocking threads), then merge.

### 2. Week-based cache model

Store data by week, not only by arbitrary `start..end` range.

Each week cache entry must contain:
- `week_monday`
- `fetched_at_utc`
- `last_refresh_utc`
- `work_items` (key, Jira URL, icon URL, summary)
- `worklogs` (date, Jira worklog URL, author, duration minutes, description plain/adf-backed)
- `commits` (date, Bitbucket commit URL, author, message without key prefix)
- `pull_requests` (created date, modification dates list, PR URL)

### 3. Period index

Maintain a per-user index of cached weeks:
- key: `<account_id>:cached_weeks`
- value: sorted unique list of Monday dates + per-week `last_refresh_utc`.

### 4. Startup warm

On server startup, warm these weeks in parallel:
- current week
- previous two weeks
- next week

Startup warm source of users:
- read valid sessions loaded from disk (`auth` session store),
- build unique active-user set by `account_id`,
- refresh OAuth access token when needed before warm calls,
- skip user when refresh fails (log warning, continue others).
- for each active user/week, fetch both Jira and Bitbucket activity during warm (not on first UI request).

One task per week per active user; bounded concurrency (max 4 per user).

### 5. User request flow

When `get_timesheet_data(start, end)` is called:
1. derive requested week Mondays,
2. check `cached_weeks`,
3. fetch only missing weeks (parallel),
4. assemble response from all requested weeks,
5. return response,
6. after response, warm adjacent weeks (period_before and period_after).

### 6. Incremental refresh loop

After a successful user request, start (or reset) a per-user refresh loop:
- interval default: 17 minutes (configurable),
- target: current week only,
- fetch only entities changed since previous loop timestamp,
- if changes exist: update week cache + push delta event to browser.

### 7. Cleanup loop

Run periodic cleanup:
- interval default: 113 minutes (configurable),
- remove week entries older than 2 months from now,
- remove orphaned index entries for deleted weeks.

## Config (new env vars)

- `CACHE_REFRESH_INTERVAL_MINUTES` (default `17`)
- `CACHE_CLEANUP_INTERVAL_MINUTES` (default `113`)
- `CACHE_RETENTION_DAYS` (default `62`)
- `CACHE_WARM_STARTUP_WEEKS_BACK` (default `2`)
- `CACHE_WARM_STARTUP_WEEKS_FORWARD` (default `1`)

Parse once at startup in `main.rs`, store in shared app state/config module.

Bitbucket env for workspace-wide mode:
- `BITBUCKET_SERVER_URL` (example: `https://bitbucket.org/uplandsoftware`)
- when project keys/URLs not configured, warm/fetch scans repos in full workspace.

## Data and API contracts

## Week cache key format

- week entry: `<account_id>:week_cache:<YYYY-MM-DD>`
- index entry: `<account_id>:cached_weeks`

## Canonical cache structs (to add in `src/api/cache.rs`)

```rust
struct CachedWeeksIndex {
    weeks: Vec<CachedWeekMeta>, // sorted by monday asc, unique
}

struct CachedWeekMeta {
    monday: NaiveDate,
    last_refresh_utc: DateTime<Utc>,
}

struct WeekCacheEntry {
    week_monday: NaiveDate,
    fetched_at_utc: DateTime<Utc>,
    last_refresh_utc: DateTime<Utc>,
    work_items: Vec<CachedWorkItem>,
    worklogs: Vec<CachedWorklog>,
    commits: Vec<CachedCommit>,
    pull_requests: Vec<CachedPullRequest>,
}

struct CachedWorkItem {
    key: String,
    jira_url: String,          // "{site_url}/browse/{key}"
    icon_url: String,
    summary: String,
}

struct CachedWorklog {
    id: String,
    issue_key: String,
    date: NaiveDate,
    jira_worklog_url: String,  // "{site_url}/browse/{key}?focusedWorklogId={id}"
    author: String,            // session.display_name for own logs
    duration_minutes: u32,
    description_text: String,
    description_adf: Option<String>,
}

struct CachedCommit {
    issue_key: String,
    date: NaiveDate,
    bitbucket_commit_url: String, // "{repo_html}/commits/{hash}"
    author: String,
    message: String,
}

struct CachedPullRequest {
    issue_key: String,
    created_date: NaiveDate,
    modification_dates: Vec<NaiveDate>, // [updated_on] or [created_on] fallback
    bitbucket_pr_url: String,           // "{repo_html}/pull-requests/{id}"
}
```

## WebSocket push channel

Add new endpoint (keep heartbeat unchanged):
- `/ws/timesheet`

Server -> browser payload:
- `kind: "timesheet_delta"`
- `week_monday`
- `changed_items` (new/updated work items)
- `changed_cells` (cell-level updates for current week)
- `server_timestamp`
- `refresh_from_utc`
- `refresh_to_utc`

Client behavior:
- prepend newly discovered work items,
- patch today-column cells matching incoming deltas,
- keep existing local unsaved edits untouched.

## Admin cache inspection endpoint

Add admin endpoint for cache introspection:
- path: `/admin/cache`
- method: `GET`
- response: JSON snapshot of full in-memory cache contents (all keys + deserialized payload when possible, else raw string)
- auth: require authenticated session (`is_authenticated(&headers)`), else `401`
- content-type: `application/json`

Response shape:

```json
{
  "generated_at_utc": "2026-07-05T09:00:00Z",
  "entry_count": 12,
  "entries": [
    {
      "key": "abc:week_cache:2026-07-06",
      "expires_at_utc": "2026-07-10T09:00:00Z",
      "kind": "week_cache",
      "value": { "...": "..." }
    }
  ]
}
```

## Implementation steps by file

### `src/api/cache.rs`

1. Add typed helpers for week-cache and index entries.
2. Add safe read/update helpers for per-user week index.
3. Add cleanup helper to prune weeks older than retention window.
4. Add `snapshot_json()` helper returning serialized cache dump for `/admin/cache`.
   - include key + expiry + payload
   - classify known payload kinds (`week_cache`, `cached_weeks`, `jira_search`, `jira_worklogs`, `timesheet_data`)
   - keep unknown payload as string field (`raw_value`)

### `src/api/jira.rs`

1. Extract reusable week-fetch function:
   - input: `(creds, week_monday)`
   - output: week-level Jira data + refresh timestamp.
2. Keep existing write invalidation behavior; extend to week index invalidation where needed.
3. Add incremental fetch helpers:
   - work items: JQL `updated >= "<last_refresh_utc>" AND worklogAuthor = "<email>"` for current week candidates.
   - worklogs: fetch per-issue worklogs and keep entries with `started` date in target week and `updated` (if present) >= `last_refresh_utc`; otherwise fallback to full per-issue pull when Jira response lacks update timestamp.

### `src/api/bitbucket.rs`

1. Add week-fetch function returning commit + PR data with URLs and timestamps needed for deltas.
2. Add incremental mode:
   - commits: stop pagination once commit date < week start, include only entries with date >= `last_refresh_date`.
   - PRs: include only `updated_on` (or `created_on` fallback) >= `last_refresh_utc`.
3. Preserve existing key extraction/summary fallback behavior.

### `src/components/timesheet_view.rs`

1. Replace direct range-only fetch path with:
   - resolve missing weeks,
   - fetch missing weeks in parallel,
   - assemble period response from cached weeks.
2. Keep existing response shape (`TimesheetData`) so UI remains compatible.
3. After response, trigger background adjacent-period warm.

### `src/main.rs`

1. Initialize caching config from env.
2. Start startup warm manager task:
   - load active authenticated users from session store,
   - refresh tokens before use,
   - spawn warm jobs for each active user,
   - each warm job calls Jira + Bitbucket for same period.
3. Start periodic cleanup task.
4. Add `/ws/timesheet` route and connection registry for per-user subscribers (`account_id -> Vec<Sender<DeltaEvent>>`).
5. Track refresh tasks per user (`account_id -> JoinHandle<()>`) so a new request cancels/replaces the prior loop.
6. Add route `GET /admin/cache`:
   - auth-guard with existing session check logic
   - on success return `axum::Json<serde_json::Value>` from `crate::api::cache::snapshot_json()`
   - on snapshot failure return `500` + error body

### `src/connection.rs` + client wiring

1. Keep heartbeat socket as-is for status indicator.
2. Add second socket subscription for timesheet delta events.
3. Apply deltas into existing reactive signals without full reload.

## Concurrency and safety rules

- Use `tokio::spawn` + bounded concurrency (`Semaphore` or `FuturesUnordered` with cap).
- Never block request thread waiting for non-critical warm jobs.
- If one source (Jira/Bitbucket) fails for a week, keep partial data and log structured warning.
- Do not drop existing cache invalidation after worklog create/update/delete.

## Acceptance criteria

1. First request for uncached multi-week period fetches missing weeks only once.
2. Repeated request for same period is served from week cache.
3. Jira and Bitbucket fetch for a week run concurrently.
4. Startup warm fills configured surrounding weeks.
5. Refresh loop emits WebSocket deltas only when data changed.
6. Cleanup removes weeks older than retention and keeps index consistent.
7. Existing worklog CRUD still updates visible UI correctly.
8. `GET /admin/cache` returns valid JSON with cache keys and payloads for authenticated user.
9. Unauthenticated `GET /admin/cache` returns `401`.

## Verification checklist

1. `cargo leptos build` succeeds.
2. Start app with `cargo leptos serve`.
3. Load current week, then navigate backward/forward and confirm warm-cache hits in logs.
4. Trigger a Jira worklog change and confirm cache invalidation + refreshed cell.
5. Simulate refresh interval and verify WebSocket delta message updates today column.
6. Reduce retention/cleanup interval in env for local test and confirm old weeks are pruned.
7. Open authenticated browser session, call `/admin/cache`, verify JSON contains expected cache entries.
8. Call `/admin/cache` without auth cookie/token, verify `401`.
