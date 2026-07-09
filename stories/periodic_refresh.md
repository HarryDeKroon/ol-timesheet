# Periodic refresh

Goal: Users should always have up-to-date information available.

## Interval

When there are WebSocket connections between the server and one or more browser sessions, the interval is 23 minutes.
When there are no WebSocket connections, the interval prolongs to 127 minutes.
These intervals are in the server configuration, but default to these values.

## Server-side check

The server fetches all work logs for the current date from Jira, and the commits and pull requests from BitBucket. This is similar to what is done at cache warming, so separate threads for Jira and Bitbucket and merge everything together based on the work item key and if required, fetch the work item metadata from Jira.
Create a diff structure between any existing data and which items (work item, work log, commit pull request) have been created, modified or removed. Update the cache accordingly and push the diff list through the web socket connection.
The Jira/Bitbucket fetch goes per active user (as determined from the web socket connection). For each user a separate thread is spawned.

## Client side update

Upon receipt of the diff list from the server on the web socket, check if the today column is visible. if so, update the timesheet with the received changes. Any new work items are added to the top of the timesheet. Show a toast to signal when new information was processed.
