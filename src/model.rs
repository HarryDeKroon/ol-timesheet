use std::collections::HashMap;

use chrono::NaiveDate;
use serde::{Deserialize, Serialize};

// ─── User preferences (replaces the old credential-bearing Settings) ─────────

/// User-editable preferences stored per-user.  These fields are shown in the
/// Settings dialog; Jira credentials come from OAuth2 and are never entered
/// manually.
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
pub struct Settings {
    pub git_folder: String,
    #[serde(default = "default_git_poll_interval_minutes")]
    pub git_poll_interval_minutes: u32,
    #[serde(default = "default_hours_per_week")]
    pub hours_per_week: f64,
    #[serde(default = "default_hours_per_day")]
    pub hours_per_day: f64,
}

fn default_git_poll_interval_minutes() -> u32 {
    5
}

fn default_hours_per_week() -> f64 {
    40.0
}
fn default_hours_per_day() -> f64 {
    8.0
}

// ─── Per-user runtime session (server-side only) ─────────────────────────────

cfg_if::cfg_if! {
    if #[cfg(feature = "ssr")] {
        /// All runtime state for one authenticated user.
        #[derive(Clone, Debug, Serialize, Deserialize)]
        pub struct UserSession {
            /// Jira `accountId` (stable identifier used as cache-key prefix).
            pub account_id: String,
            pub email: String,
            pub display_name: String,
            pub avatar_url: String,
            /// OAuth2 Bearer access token.
            pub access_token: String,
            /// Rotating refresh token — update on every refresh.
            pub refresh_token: String,
            /// Unix timestamp (UTC) when access_token expires.
            pub expires_at: i64,
            /// Atlassian cloud instance ID.
            pub cloud_id: String,
            /// Jira site base URL, e.g. "https://uplandsoftware.atlassian.net".
            pub site_url: String,
            /// User-editable preferences (git folder, hours/day, etc.).
            pub preferences: Settings,
        }

        impl UserSession {
            pub fn jira_credentials(&self) -> crate::api::jira::JiraCredentials {
                crate::api::jira::JiraCredentials {
                    access_token: self.access_token.clone(),
                    cloud_id: self.cloud_id.clone(),
                    email: self.email.clone(),
                    account_id: self.account_id.clone(),
                }
            }
        }
    }
}

// ─── Timesheet domain types (shared between client and server) ──────────────

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct WorkItem {
    pub key: String,
    pub summary: String,
    pub icon_url: String,
    pub issue_type: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct WorklogEntry {
    pub id: String,
    pub issue_key: String,
    pub date: NaiveDate,
    pub hours: f64,
    pub comment: String,
    /// HTML rendering of the original ADF comment.  Empty when the
    /// comment was plain text or when the entry was created locally.
    #[serde(default)]
    pub comment_html: String,
    /// Raw ADF JSON of the original comment, kept for round-tripping.
    /// When the user saves without editing the comment text, this is
    /// sent back to Jira so that rich formatting is preserved.
    #[serde(default)]
    pub comment_adf: Option<String>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
pub struct TimesheetData {
    pub work_items: Vec<WorkItem>,
    pub worklogs: Vec<WorklogEntry>,
    pub hours_per_week: f64,
    pub hours_per_day: f64,
    /// Legacy field – tolerated during deserialization of cached data but no
    /// longer populated or consumed at runtime.
    #[serde(default, rename = "meeting_keys", skip_serializing)]
    pub(crate) _meeting_keys: Option<Vec<String>>,
    /// Year-to-date total hours logged by the current user per issue key.
    #[serde(default, alias = "all_time_hours")]
    pub ytd_hours: HashMap<String, f64>,
    /// Git commit messages per (issue_key, date), if available.
    #[serde(default)]
    pub git_commits: Option<std::collections::HashMap<String, Vec<String>>>,
    /// Jira site base URL, e.g. "https://uplandsoftware.atlassian.net".
    /// Used by the client to build worklog deep-link URLs.
    #[serde(default)]
    pub site_url: String,
}

impl TimesheetData {
    /// Get total hours logged for a given issue key and date.
    pub fn cell_hours(&self, key: &str, date: NaiveDate) -> f64 {
        self.worklogs
            .iter()
            .filter(|w| w.issue_key == key && w.date == date)
            .map(|w| w.hours)
            .sum()
    }

    /// Get all worklog entries for a given issue key and date.
    pub fn cell_worklogs(&self, key: &str, date: NaiveDate) -> Vec<&WorklogEntry> {
        self.worklogs
            .iter()
            .filter(|w| w.issue_key == key && w.date == date)
            .collect()
    }

    /// Total hours for a given date across all work items.
    pub fn day_total(&self, date: NaiveDate) -> f64 {
        self.worklogs
            .iter()
            .filter(|w| w.date == date)
            .map(|w| w.hours)
            .sum()
    }

    /// Total hours for a given issue key across all dates in the dataset.
    pub fn item_total(&self, key: &str) -> f64 {
        self.worklogs
            .iter()
            .filter(|w| w.issue_key == key)
            .map(|w| w.hours)
            .sum()
    }

    /// Year-to-date total hours logged by the current user for a given issue key.
    pub fn item_ytd_total(&self, key: &str) -> f64 {
        self.ytd_hours.get(key).copied().unwrap_or(0.0)
    }

    /// Weekend total for a given issue key in the week starting at `monday`.
    pub fn weekend_hours(&self, key: &str, monday: NaiveDate) -> f64 {
        let saturday = monday + chrono::Duration::days(5);
        let sunday = monday + chrono::Duration::days(6);
        self.cell_hours(key, saturday) + self.cell_hours(key, sunday)
    }

    /// Weekend total across all items for a week starting at `monday`.
    pub fn weekend_total(&self, monday: NaiveDate) -> f64 {
        let saturday = monday + chrono::Duration::days(5);
        let sunday = monday + chrono::Duration::days(6);
        self.day_total(saturday) + self.day_total(sunday)
    }

    /// Week total for a given week starting at `monday`.
    pub fn week_total(&self, monday: NaiveDate) -> f64 {
        (0..7)
            .map(|i| self.day_total(monday + chrono::Duration::days(i)))
            .sum()
    }

    /// Week total for a specific item.
    pub fn item_week_total(&self, key: &str, monday: NaiveDate) -> f64 {
        (0..7)
            .map(|i| self.cell_hours(key, monday + chrono::Duration::days(i)))
            .sum()
    }
}
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum ConnectionStatus {
    Online,
    Waiting,
    Offline,
}
