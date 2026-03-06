# Heartbeat

We want to add a heartbeat check, so we can notify the user when the connection to the server becomes unavailable.

## UI

We already have an indicator (`span.circle.green`) in `div.bottom-nav`.
The indicator is:

- green when the connection is available
- orange when a request is in progress
- red when the server is no longer responding

The cell click listener should only respond when the connection is available.
Similarly, the cell pop-up inputs and buttons should only be enabled when the connection is available.

## I18n requirements (must comply with `instructions.md`)

This story must comply with project I18n rules:

- **No literal user-facing strings** in `view!` blocks.
- **All user-facing strings and attributes** must use translation keys, including:
   - visible text
   - tooltips/title attributes
   - placeholders
   - aria labels / accessibility text
   - status messages
- Use `i18n.get().t(keys::...)` for all heartbeat-related UI text.
- This includes heartbeat indicator status text such as:
   - connected
   - syncing / request in progress
   - disconnected
   - connection unavailable (if shown as toast/banner)
- Server-rendered output must also avoid literal strings.

### Suggested new i18n keys (heartbeat)

Add keys (names can be adapted to your existing key naming conventions):

- `CONNECTION_CONNECTED`
- `CONNECTION_SYNCING`
- `CONNECTION_DISCONNECTED`
- `CONNECTION_UNAVAILABLE`
- `HEARTBEAT_INDICATOR_TITLE` (optional umbrella label)
- `HEARTBEAT_STATUS_ARIA` (optional for `aria-live` message formatting)

## Implementation

The server provides a websocket that we can use to check the connection.

When the websocket is open, the connection is available.
When the websocket is closed, the connection is unavailable.
The client can listen to the websocket `open`/`close` events to update the indicator.

When the client calls the server API, the indicator turns orange while the request is in progress, and turns green when the response is received.

### Plan

## 1) Define a single source of truth for connection state

Create a small connection-state module/store with:

- `isSocketOpen: boolean`
- `inFlightRequestCount: number` (preferred over simple boolean so concurrent requests are safe)
- derived `connectionStatus: 'online' | 'requesting' | 'offline'`

Derivation rules:

1. If socket is **closed** → `offline` (red), regardless of request count.
2. Else if socket open and `inFlightRequestCount > 0` → `requesting` (orange).
3. Else socket open and no requests → `online` (green).

This keeps behavior deterministic and prevents flicker/race issues.

## 2) Websocket lifecycle integration

Implement websocket initialization at app startup (or when authenticated/session-ready), then listen for:

- `open` → set `isSocketOpen = true`
- `close` → set `isSocketOpen = false`
- (optional) `error` → log/telemetry; usually followed by `close`

Reconnection strategy:

- exponential backoff with cap + reset on successful open

Even without aggressive heartbeat pings, the open/close event model here is enough per our story.

## 3) API request instrumentation (orange state)

Wrap all API calls in a shared request helper/interceptor:

- before request: increment `inFlightRequestCount`
- in `finally`: decrement count (never below zero)

This guarantees orange during every request and return to green when complete (assuming socket still open).
If request fails due to network/server issues, still decrement in `finally`.

If you already have a central HTTP client, this is the best hook point.

## 4) Indicator UI wiring (`span.circle.green`)

Refactor indicator rendering so class/style is driven by `connectionStatus`, not hardcoded class names.

Example class mapping:

- `online` → `circle green`
- `requesting` → `circle orange`
- `offline` → `circle red`

Avoid directly mutating DOM in multiple places.
One renderer/subscription should own indicator updates.

### I18n for indicator wiring

Any indicator tooltip/title/label must use translation keys, for example:

- `title={i18n.get().t(keys::CONNECTION_CONNECTED)}`
- `title={i18n.get().t(keys::CONNECTION_SYNCING)}`
- `title={i18n.get().t(keys::CONNECTION_DISCONNECTED)}`

Do not hardcode literal strings like `"Connected"` / `"Syncing..."` / `"Disconnected"` in component code.

## 5) Guard user interactions when offline

### 5a) Cell click listener

Update click handler guard:

- if `connectionStatus === 'offline'`: no-op (optionally show non-blocking translated toast/banner)
- else proceed

Story wording suggests orange is still connected, so allow interaction during `requesting` unless product decides otherwise.

If a user-facing message is shown, it must use i18n key (e.g. `CONNECTION_UNAVAILABLE`).

### 5b) Cell popup controls

When popup opens (or on status changes while open), set:

- inputs `disabled = offline`
- action buttons `disabled = offline`

Also visually indicate disabled state for accessibility/usability.

Any accessibility labels, helper text, and tooltips must use i18n keys.

## 6) Optional UX polish (recommended)

- Show a status text/tooltip near indicator (translated):
   - connected
   - syncing
   - disconnected
- Debounce very short orange transitions (e.g., <150 ms) if flicker is annoying.
- Announce status changes via `aria-live` (translated).

## 7) Edge cases to explicitly handle

- **Concurrent requests**: count-based handling prevents incorrect green when one request finishes early.
- **Socket closes during request**: final state should be red (offline wins).
- **Reconnect after offline**: red → green automatically on open (or orange if requests active).
- **Unexpected count drift**: clamp at `Math.max(0, count)` and log if negative attempted.
- **Page hidden/resume**: ensure websocket/reconnect logic handles browser lifecycle cleanly.

## 8) Testing plan

### Unit tests (state logic)

- status derivation matrix:
   - open + 0 => green
   - open + >0 => orange
   - closed + any => red
- request counter increments/decrements correctly, including error paths.
- offline precedence over requesting.

### Integration tests

- mock websocket open/close and verify indicator class changes.
- simulate API request and verify orange during request, green after completion.
- verify click handler blocked when offline.
- verify popup controls disabled when offline and re-enabled on reconnect.
- verify heartbeat-related text/attributes come from i18n keys (no literal strings in rendered UI).

### Manual QA checklist

1. Start connected: indicator green.
2. Trigger save/load: orange then green.
3. Kill server/socket: indicator red.
4. Click cell while red: no response.
5. Open popup while red: controls disabled.
6. Restore server: indicator returns green; controls re-enable.
7. Change language: all heartbeat-related labels/tooltips/messages localize correctly.

## 9) Suggested implementation sequence (low-risk)

1. Add connection state module + derivation.
2. Hook websocket open/close into state.
3. Hook API wrapper/interceptor into request count.
4. Bind indicator class to derived status.
5. Replace all heartbeat literal text/attributes with i18n keys.
6. Add interaction guards (cell click + popup controls).
7. Add/extend tests.
8. Add optional translated status text / aria-live polish.
