# User report

Goal is to add a reporting module to the timesheet, which will give the user an insight of their monthly progress as compared to contract hours, as well as the ratio between billable and overhead hours spent

## Configuration changes

Add a group to the settings dialog, called Reporting

- "non billable project(s)": text input for a list of project names e.g., TIM
- "meetings",
- "local holidays",
- "planned time off",
- "study": all text inputs for a list of work item keys

These are all optional. The labels are of course according to the i18n rules.

## Server endpoint

Add a new endpoint on the server "/report/:year" which returns the aggregated work log minutes in JSON format for the given year.
For the active user, aggregate all work logs in minutes per project, per week, starting from the first week of the given year until today, or Dec 31 of that year, whichever comes first

### Response template

(the colons ':' denote places where the previous block may be repeated multiple times)

```json
{
    "billable": {
        "SHARED": {
            "20251229": 2302,
            "20260105": 2014,
            "20260112": 2345,
            "20260119": 2255,
            "20260126": 2099,
            "20260212": 2400,
            	:
        }
    },
    "non-billable": {
        "20251229": {
            "holidays": 480,
            "meetings": 123,
            "other": 211,
            "pto": 960,
            "study": 0
        },
        		:
    }
}
```

## UI

Before the "refresh Cache" button on the bottom row comes a new button: title="User report" and like the other buttons it's "label" is a unicode character or SVG which represents a report.
When clicked, an overlay pops over the timesheet view, 70% x 70% of the viewport width

- the first row shows some filters and dropdowns:
   - a checkbox with the names of the available projects, as well as for each of the non-billable categories. The stacked bar graph and the pie chart only show stacks or slices for the selected items.
   - a period dropdown
      - Week
         - the selected period starts with the first week of the month before the current month and continues to the last week of the current month. Left and right of the graphs below are selectors for the previous and next month. The stacked bar graph shows a bar per week. The y-axis of the stacked bar graph goes from 0 to 7 days with a thin grid line per day and a thicker one on day 5.
      - Month
         - the selected period starts with the first week of the given (default=current) year and continues to the last week of the given year. Left and right of the graphs below are selectors for the previous and next year (next only if not current year). The stacked bar graph shows a bar per month. The y-axis of the bar graph goes from 0 to 25 days with a thin grid line per five days and a thicker for the 10 and 20 days line
   - The total number of hours planned time off for the selected period
- The top half shows a stacked bar graph (SVG) of the aggregated work log days for the selected period. Each stack hast the number of days in the center center position of the stack and the number of hours in the title attribute.
   - the first stacks are the billable ones. each project has it's own color. Stacks of 0 are not added. (Group the stacks under "billable")
   - The second group is "none-billable" and have a stack per selected item of the enum {holidays, meetings, pto, study, other} A work log for a work item that starts with one of the non-billable project names (e.g., TIM), but is not mentioned under one of the four other categories are added to the "other" stack. These stack get each a different tint of the color for non-billable.
- The bottom half shows a pie chart (SVG) of the cummilative work log days. Again two groups: billable and non-billable. Each pie slice gets the percentage of the total amount of hours for that slice in the center center position and the total amount of hours in the title attribute. Only calculate totals for the selected items, including the grand total.
   - The billable group shows a slice per billable project, with the cummulative total for that project for the selected period
   - the non-billable group shows a slice for each of the selected non-billable items

### Styling through style sheets

Use a separate style sheet for the report. All colors, line thicknesses and other styling must go through this stylesheet, no inline.

### Day calculation

The number of days are calculated by taking the (total) number of minutes, divided by the number of hours (may be fractional e.g. 7.5) per day (from the user settings) times 60 and rounded to the nearest half.
