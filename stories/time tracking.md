# Time tracking

Add the option to track time for work items. Instead of entering durations manually, the user can start and stop timers for each work item.

## Approach

When the user clicks on a cell that corresponds to a work item and a day, the cell pop-up opens to present the work log items and allow for modifications. Currently, only one cell editor can be active at a time.
When the cell is in the Today column, (also true for weekends, of course), the rows all get an extra button (similar to a play button) that allows the user to start a timer for that work item. The timer can be paused by clicking the button (now looking like a pause button) again, or stopped by a new button (similar to a stop button). Paused timers can be resumed by clicking the pause button again. Since now multiple cell editors can be active simultaneously, the cell editor pop-up must be moveable, so the user can position it where they want it.

### Timing

When a timer is started, it parses the current value of the duration input on the same line and starts a timer in the browser for 2.5 minutes. After that timer ends, the duration value is updated by 5 minutes, and the timer starts again for now 5 minutes. This process repeats until the timer is stopped. When the timer is stopped, the duration value is not updated, as that has already been accounted for in the previous timer interval.
When a timer is paused, the duration value is not updated, but the remaining time of it is recorded. When the timer is resumed, a new timer is started for the remaining time. After that timer ends, the duration value is updated by 5 minutes, and the timer starts again for 5 minutes, and so on.

#### One active timer

Only one timer can be active at a time. When a new timer is started, any existing active timer is paused.

## UI

The existing validation of the cell pop-up still applies, as well as the logic for adding a new, blank work log entry line. Closing the cell pop-up stops all active or paused timers.

## Implementation Plan

### 1. UI Changes

- **Cell Pop-up**: Refactor to allow multiple pop-ups to be open and moveable.
- **Timer Controls**: Add play, pause, and stop buttons to each work item row in the Today column (and weekends). Use icons for play/pause/stop and add tooltips (mind I18n) for clarity.
- **Timer State Display**: Show timer state (running, paused, stopped) and elapsed time.

### 2. Timer Logic

- **Single Active Timer**: Ensure only one timer is active at a time. Starting a new timer pauses any currently running timer.
- **Timer Intervals**:
   - On start, parse the current duration and begin a timer for 2.5 minutes.
   - After 2.5 minutes, increment duration by 5 minutes and start a new 5-minute timer.
   - Repeat until stopped.
- **Pause/Resume**:
   - Pausing records remaining time.
   - Resuming starts a timer for the remaining time, then continues with 5-minute intervals.
- **Stop**:
   - Stopping does not update duration (already handled by previous intervals).
   - Closing the pop-up stops all timers.

### 3. State Management

- **Timer State**: Store timer state (running, paused, stopped, remaining time) per work item.
- **Duration Updates**: Update the duration input in the UI as timers complete intervals.
- **Pop-up Management**: Track which pop-ups are open and their positions.

### 4. Validation & Integration

- **Existing Validation**: Ensure cell pop-up validation and logic for adding new work log entries remain intact.
- **API Integration**: If durations are persisted, update the backend as durations change.

### 5. Testing

- **Unit Tests**: Test timer logic, state transitions, and duration updates.
- **UI Tests**: Test pop-up movement, timer controls, and validation.
