use std::collections::HashMap;

use chrono::NaiveDate;
use serde::{Deserialize, Serialize};

// ─── User Settings ──────────────────────────────────────────────────────────

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
pub struct Settings {
    pub email: String,
    pub upland_jira_token: String,
    /// Legacy field – ignored at runtime but kept so that existing
    /// `settings.json` files that still contain it deserialize without error.
    #[serde(default, rename = "meeting_keys", skip_serializing)]
    pub(crate) _meeting_keys: Option<String>,
    pub bitbucket_username: String,
    pub bitbucket_app_password: String,
    pub ol_jira_username: String,
    pub ol_jira_password: String,
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

// ─── Server-side persistence ────────────────────────────────────────────────

cfg_if::cfg_if! {
    if #[cfg(feature = "ssr")] {
        fn config_dir() -> std::path::PathBuf {
            let dirs = directories::ProjectDirs::from("com", "objectiflune", "timesheet")
                .expect("Could not determine config directory");
            let config_dir = dirs.config_dir().to_path_buf();
            std::fs::create_dir_all(&config_dir).ok();
            config_dir
        }

        pub fn load_settings() -> Settings {
            let path = config_dir().join("settings.json");
            if path.exists() {
                let data = std::fs::read_to_string(&path).unwrap_or_default();
                serde_json::from_str(&data).unwrap_or_default()
            } else {
                Settings::default()
            }
        }

        pub fn save_settings(settings: &Settings) -> Result<String, String> {
            let dir = config_dir();
            let data = serde_json::to_string_pretty(settings).map_err(|e| e.to_string())?;
            std::fs::write(dir.join("settings.json"), data).map_err(|e| e.to_string())?;

            // Generate and store a new token
            let token = uuid::Uuid::new_v4().to_string();
            std::fs::write(dir.join("token"), &token).map_err(|e| e.to_string())?;
            Ok(token)
        }

        pub fn validate_token(token: &str) -> bool {
            let path = config_dir().join("token");
            if path.exists() {
                if let Ok(stored) = std::fs::read_to_string(&path) {
                    return stored.trim() == token.trim();
                }
            }
            false
        }
    }
}
