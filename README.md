# OL Timesheet

Inspired by [Sander's Java tool](https://bitbucket.org/uplandsoftware/dev-day-experiments/src/timesheet-SvdB/), here another version, that allows to maintain worklog entries that have comments. Also each week displays the total time registered, as well as the total time spent on each work item.

![Screenshot](https://github.com/HarryDeKroon/ol-timesheet/blob/master/screenshots/OL%20Timesheet.png "Sample view")

## Usage

The main view shows a table of worklog entries, with columns for each working day, plus one for the weekend and a week total.
Each worklog entry can be edited by clicking on it, and a popup allows adding, removing or modifying comments. Comments that were saved in [Atlassian Document Format](https://developer.atlassian.com/cloud/jira/platform/apis/document/structure/) are displayed as rich text, but are readonly. Next to each comment is a direct link to the comment in Jira.

The duration of each worklog entry is displayed as a human-readable string (e.g. "2h 30m") in the popup window that appears when you edit a worklog entry. Longer durations may also include "d" for days and "w" for weeks. The values for days and weeks depend on the user settings for 'hours per week' and 'hours per day'. (e.g. if 'hours per week' is 40 and 'hours per day' is 8, then a duration of 32 hours will be displayed as "4d", whereas for 'hours per week' is 30 and 'hours per day' is 7.5, a duration of 40 hours will be displayed as "1w 1d 2h 30m").

### Time tracking

To track time, simply click on a worklog entry **in the today column** to edit it. Next to the time input field, there is a button that allows you to start or stop tracking time. When tracking is started, the button changes to a pause button, and the time is automatically updated as you work. When tracking is paused, the button changes back to a start button, and time tracking is suspended until resumed by pressing the start button again.

Only one worklog entry can be tracked at a time. If you start tracking a new entry, any existing tracking will be paused.

**Note:** Just as manual time tracking, time tracking is not automatically saved. You must click the "Save" button to save your changes.

## TO DO
   # Multi user
   # OAuth2/SSO
   # CSV/Excel export
   # Offline worklog queue for intermittent connectivity
   # Light/Dark mode support
   # Keyboard navigation in timesheet grid

