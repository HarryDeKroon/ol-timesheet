# OL Timesheet

Inspired by [Sander's Java tool](https://bitbucket.org/uplandsoftware/dev-day-experiments/src/timesheet-SvdB/), here another version, that allows to maintain worklog entries that have comments. Also each week displays the total time registered, as well as the total time spent on each work item.

![Screenshot](https://github.com/HarryDeKroon/ol-timesheet/blob/develop/screenshots/OL%20Timesheet%2020260706.png "Sample view")

## Usage

The main view shows a table of worklog entries, with columns for each working day, plus one for the weekend and a week total.
Each worklog entry can be edited by clicking on it, and a popup allows adding, removing or modifying comments. Comments that were saved in [Atlassian Document Format](https://developer.atlassian.com/cloud/jira/platform/apis/document/structure/) are displayed as rich text, but are readonly. Next to each comment is a direct link to the comment in Jira.

The duration of each worklog entry is displayed as a human-readable string (e.g. "2h 30m") in the popup window that appears when you edit a worklog entry. Longer durations may also include "d" for days and "w" for weeks. The values for days and weeks depend on the user settings for 'hours per week' and 'hours per day'. (e.g. if 'hours per week' is 40 and 'hours per day' is 8, then a duration of 32 hours will be displayed as "4d", whereas for 'hours per week' is 30 and 'hours per day' is 7.5, a duration of 40 hours will be displayed as "1w 1d 2h 30m").

### Time tracking

To track time, simply click on a worklog entry **in the today column** to edit it. Next to the time input field, there is a button that allows you to start or stop tracking time. When tracking is started, the button changes to a pause button, and the time is automatically updated as you work. When tracking is paused, the button changes back to a start button, and time tracking is suspended until resumed by pressing the start button again.

Only one worklog entry can be tracked at a time. If you start tracking a new entry, any existing tracking will be paused.

**Note:** Just as manual time tracking, time tracking is not automatically saved. You must click the "Save" button to save your changes.

## Bitbucket webhooks (optional)

By default the server polls Bitbucket for new commits / pull-request activity every 23 minutes (127 when idle). To get near-instant updates instead, Bitbucket can push `repo:push` and `pullrequest:*` events to the app via a webhook.

Because the app runs on `localhost:8081`, Bitbucket Cloud needs a tunnel to reach it:

- **Cloudflare Tunnel** (recommended — stable URL): `cloudflared tunnel --url http://localhost:8081` with a named tunnel, or
- **ngrok**: `ngrok http 8081` (free-tier URLs change on every restart; the app re-registers the webhook automatically on startup, so restart the app after restarting the tunnel).

Setup:

1. Set `WEBHOOK_PUBLIC_URL` to the tunnel's public base URL (e.g. `https://my-tunnel.example.com`) in the environment or `.env`.
2. Configure Bitbucket workspace access with `BITBUCKET_WORKSPACE` (or `BITBUCKET_SERVER_URL`, for example `https://bitbucket.org/uplandsoftware`). Create a dedicated Bitbucket service account (or bot user) and configure the server with its credentials via `BITBUCKET_API_USER` and `BITBUCKET_API_TOKEN`:
   - **From July 28, 2026 onward (recommended):** Use an **Atlassian API token**. Set `BITBUCKET_API_USER` to the service account's **email address** and `BITBUCKET_API_TOKEN` to the API token generated at [id.atlassian.com/manage-profile/security/api-tokens](https://id.atlassian.com/manage-profile/security/api-tokens).
   - **Before July 28, 2026 (legacy):** Use an app password. Set `BITBUCKET_API_USER` to the service account's **Bitbucket username** and `BITBUCKET_API_TOKEN` to the app password.

   The token/password needs `read:webhook:bitbucket` and `write:webhook:bitbucket` scopes in addition to repository/pullrequest/workspace/project read.

3. Start the tunnel, then the app. On startup it registers (or repoints) a webhook — workspace-level when the token permits, otherwise per repository on the repos where the configured Bitbucket API user has **admin** access (the permission level Bitbucket requires for webhook management) — identified by a marker `ol-timesheet:<hostname>` so each developer machine manages only its own hooks. Repos without a hook remain covered by fallback polling.

A random URL path token and an HMAC-SHA256 signature secret (persisted in `webhook.json` next to the app's config files; override the secret with `WEBHOOK_SECRET`) protect the endpoint. Incoming events are debounced for ~20 seconds and then trigger the same refresh/diff/WebSocket pipeline as the periodic poll.

While webhooks are operational, polling is demoted to a safety-net cadence (`PERIODIC_REFRESH_FALLBACK_MINUTES`, default 127) in case the tunnel drops, and a catch-up refresh runs whenever a browser session (re)connects.

**Not covered:** Jira worklog changes still rely on polling (Jira webhooks require site-admin rights).

## Jenkins test-result links (optional)

You can enable per-cell Jenkins test-result links (with a blue `T` badge) for Bitbucket commit activity.

Required environment variables:

- `JENKINS_BASE_URL` (example: `https://windows-jenkins.upland-dev.com`)
- `JENKINS_ROOT_PATH` (optional, default: `/job/ObjectifLune/job`)

Optional (for private Jenkins instances):

- `JENKINS_API_USER`
- `JENKINS_API_TOKEN`

When enabled, the server tries to resolve each matching commit to a finished Jenkins build and links to its `testReport` page when available.

## TO DO

- CSV/Excel export
- Light/Dark mode support
- Keyboard navigation in timesheet grid
