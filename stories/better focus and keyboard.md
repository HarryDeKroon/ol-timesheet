# UI overhaul and keyboard support improvement

## Page load

On first page load, the following states can be distinguished:

1. No token -> OAuth2 login sequence to establish token
2. No user preferences -> Show settings dialog to enter preferences; cannot continue until completed
3. normal operation: valid token and user preferences -> show timesheet

## UI changes

### General layout

- H1 no longer rendered, display "Timesheet {user_name}" in HEAD > TITLE
- navigation and status go to the top row, stylised as the Ribbon in Microsoft Office products
- main view displays either timesheet view or user report view

#### Ribbon

The navigation bar shows three types of controls - left -> Menu options, like Timesheet and Report - middle -> navigation like previous/next period - right -> user avatar and status
Menu options and navigation depend on the active view, avatar and status are always visible.
The ribbon can be collapsed or expanded. When collapsed, only avatar and status are available, plus an small expand button at the far right. When expanded, a small collapse button is at the far right.

The user avatar comes from Jira. When clicked it opens a dropdown with Settings option (gear wheel symbol) and Logout (door symbol)

### Settings dialog

The settings dialog opens as a modal dialog: 80% of the available viewport, but the rest op the UI is covered in a semi-transparent gray shaded box. No keyboard or mouse events are passed to the other views
Add a UI group to the top of the settings dialog with a Language option. That option is the current flag button and changes the UI language immediately (no save required)
The settings dialog has the same Save and close buttons as it has now. Both close the dialog (and removes the overlay), so that operation continues as before opening the dialog.
The close button may not appear when the settings are displayed because no user preferences are available yet

### Timesheet view

This is the timesheet view as we already have. Navigation shows Report button and both refresh buttons left and the current week navigator in the center

#### Keys

- Alt-L -> Focus to work item cell
- Alt-P -> previous week
- Alt-N -> next week
- Alt-D -> date picker
- Alt-T -> go to today (if today column is not visible)
- Alt-S -> Settings
- Alt-X -> Logout
- Alt-R -> Report view
- Alt-F -> periodic refresh

### Report view

As it is now, but the .report-controls and .report-period-nav move to the center of Navigation in the Ribbon. Ribbon shows only a new Timesheet button on the left. Report view uses all of the main area, the same as the Timesheet table does. There is no longer a close button.

#### Keys

- Alt-P -> previous month/year
- Alt-N -> next month/year
- Alt-D -> activate period drop-down
- Alt-T -> go to current month/year
- Alt-S -> Settings
- Alt-X -> Logout
- Alt-W -> Timesheet view
