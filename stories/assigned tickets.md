# Assigned Tickets to replace meeting keys

## Goal

We support so called "meeting keys", that is a list of comma separated work item keys, that are stored in the settings.json. The work items on this list are always presented in the time sheet view and sorted after the list of work items with work log entries on them.
Since the work items in this list always have status "In Progress", and we actually want to always include these active work items in the time sheet view, the "meetings key" logic needs to be replaced with a different approach.

## Approach

In fetch_work_items (jira.rs), we load all work items assigned to the user and filter them by status. Instead of using a "meetings key" list, we should include the result from the following jql query: 'assignee = {username} and status IN ("Code Review", "In Progress")'. Here the {username} placeholders must be replaced with the actual email of the user (which is part of the settings.json). The literal strings must not be translated!

Step 3 of prefetch_week (jira.rs) should sort the work items by only their key. All other sorting must be removed.

### Meeting keys removal

All references to the "meetings key" list should be removed from the entire codebase. This is also true for presenting the total duration of work log entries per work item. (So, now all work items have a total duration)

---

## Detailed implementation and validation plan

### 1) Remove meeting keys from runtime domain model

- Remove `meeting_keys` from settings-driven business logic.
- Remove `meeting_key_list()` and any helpers that derive/consume this list.
- Remove `meeting_keys` from `TimesheetData` and all call sites.
- Keep deserialization robust for existing `settings.json` files that may still contain `meeting_keys` (ignore legacy field instead of failing).

### 2) Update Jira work-item retrieval strategy

In `fetch_work_items` (`jira.rs`):

- Keep current retrieval of work items with work logs in the selected period.
- Add retrieval of assigned active tickets with JQL:

   `assignee = {email} and status IN ("Code Review", "In Progress")`

- Replace `{email}` using the user email from `settings.json`.
- Keep Jira status literals exactly as shown (`"Code Review"`, `"In Progress"`); do not localize or translate these values.
- Merge both result sets and deduplicate by issue key.

### 3) Simplify sorting behavior

In `prefetch_week` step 3 (`jira.rs`):

- Sort work items by key only.
- Remove all previous conditional sorting/grouping rules (including meeting-key-first/last behavior).
- Ensure consistent deterministic ordering across fetch and prefetch paths.

### 4) Remove meeting-specific UI behavior

In the timesheet view:

- Remove any logic that treats meeting-key rows differently.
- Ensure total logged duration is shown for all work items.
- Remove variables/flags used only for meeting-key conditions.

### 5) Remove meeting keys from settings UI and i18n entries

- Remove meeting keys input/control from settings dialog.
- Remove now-unused i18n keys and placeholders tied to meeting keys.
- Ensure all remaining user-facing strings still follow project i18n rules (no literal UI strings in `view!` output).

### 6) Align caching/prefetch with new inclusion rules

- Verify that cache population and prefetch include assigned active tickets as part of the assembled result.
- Ensure deduplication occurs before cache write.
- Confirm no stale assumptions remain in cache-read paths about meeting-key behavior.

### 7) Project-wide cleanup pass

Search and remove all references to:

- `meeting_keys`
- `meeting key(s)` logic comments
- any display/ordering branches tied to meetings

Also update comments/docs in code where behavior description changed.

### 8) Validation plan

#### Unit / logic checks

- Assigned-active JQL result is included in returned work items.
- Merge+dedup by key works when an item appears in both sets.
- Sorting is key-only and stable.

#### Integration checks

- Timesheet displays active assigned items even without work logs in period.
- All rows show total duration consistently.
- Settings dialog no longer exposes meeting keys.
- Existing legacy `settings.json` containing `meeting_keys` still loads safely.

#### Manual QA checklist

1. User has active assigned ticket in status `In Progress` with no period work logs → ticket appears.
2. User has active assigned ticket in `Code Review` → ticket appears.
3. Ticket appearing in both sources appears once.
4. Row totals appear for every ticket.
5. Ordering is by key only.
6. Meeting-key field is absent from settings UI.
7. App loads correctly with old settings file containing `meeting_keys`.

### 9) Suggested implementation sequence (low-risk)

1. Implement Jira fetch merge/dedup update.
2. Implement key-only sorting in prefetch path.
3. Remove model references (`meeting_keys`, helper methods, data struct fields).
4. Remove UI branches and settings field.
5. Clean i18n entries and comments.
6. Run compile/test/QA checklist and final grep cleanup.
