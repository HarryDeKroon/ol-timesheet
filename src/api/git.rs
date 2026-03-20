use chrono::NaiveDate;
use leptos::prelude::ServerFnError;
use leptos::*;
use std::collections::HashMap;

cfg_if::cfg_if! {
    if #[cfg(feature = "ssr")] {
        use anyhow;
        use regex::Regex;
        use std::collections::HashSet;
        use std::process::Command;

        struct GitCommit {
            message: String,
            date: String,
            name: String,
            email: String,
        }

        fn fetch_git_commit_data(
            git_folder: &str,
            start: NaiveDate,
            end: NaiveDate,
        ) -> anyhow::Result<Vec<GitCommit>> {
            let output = Command::new("git")
                .arg("rev-list")
                .arg("--remotes")
                .arg("--no-merges")
                .arg("--date=short-local")
                .arg("--format=%s%n%ad%n%aN%n%cE")
                .arg(format!("--since={}T00:00", start))
                .arg(format!("--until={}", end))
                .current_dir(git_folder)
                .output()?;

            if !output.stderr.is_empty() {
                let err = String::from_utf8_lossy(&output.stderr);
                log::warn!("[fetch_git_commit_data] git stderr: {}", err);
            }

            let stdout = String::from_utf8_lossy(&output.stdout);
            let mut lines = stdout.lines();
            let mut commits = Vec::new();

            while lines.next().is_some() {
                let message = lines.next();
                let date = lines.next();
                let name = lines.next();
                let email = lines.next();

                if let (Some(message), Some(date), Some(name), Some(email)) = (message, date, name, email) {
                    commits.push(GitCommit {
                        message: message.to_string(),
                        date: date.to_string(),
                        name: name.to_string(),
                        email: email.to_string(),
                    });
                }
            }
            Ok(commits)
        }

        fn extract_work_item_key(message: &str) -> Option<String> {
            let key_regex = Regex::new(r"^(\w+-\d{3,6})").ok()?;
            key_regex
                .captures(message)
                .and_then(|caps| caps.get(1).map(|m| m.as_str().to_string()))
        }

        /// Fetches Git commit messages for the given work item keys and users within the date range.
        /// Returns a map from (issue_key, date) to a vector of commit messages (in commit order).
        ///
        /// - `git_folder`: Path to the Git workspace.
        /// - `work_item_keys`: Set of work item keys to match (e.g., "SHARED-12345").
        /// - `users`: Set of user names or emails to match as commit authors.
        /// - `start`: Start date (inclusive).
        /// - `end`: End date (inclusive).
        pub fn fetch_git_commits(
            git_folder: &str,
            work_item_keys: &HashSet<String>,
            users: &HashSet<String>,
            start: NaiveDate,
            end: NaiveDate,
        ) -> anyhow::Result<HashMap<String, Vec<String>>> {
            let commits = fetch_git_commit_data(git_folder, start, end)?;

            let mut map: HashMap<String, Vec<String>> = HashMap::new();

            for commit in commits {
                if users.contains(&commit.name) || users.contains(&commit.email) {
                    if let Ok(date) = NaiveDate::parse_from_str(&commit.date, "%Y-%m-%d") {
                        if let Some(key) = extract_work_item_key(&commit.message) {
                            if work_item_keys.contains(&key) {
                                let map_key = format!("{}:{}", key, date);
                                let key_strip_regex =
                                    Regex::new(&format!(r"(?i)^{}[:\\-\\s]+", regex::escape(&key)))
                                        .unwrap();
                                let cleaned_message = key_strip_regex
                                    .replace(&commit.message, "")
                                    .trim_start()
                                    .to_string();
                                map.entry(map_key).or_default().push(cleaned_message);
                            }
                        }
                    }
                }
            }
            Ok(map)
        }
    }
}
/// Server function to check for new git commits not in known_keys, returning key/message pairs.
/// Returns a map from work item key to the first commit message found for that key.
#[server(CheckForNewGitCommits, "/api")]
pub async fn check_for_new_git_commits(
    known_keys: Vec<String>,
    start: NaiveDate,
    end: NaiveDate,
) -> Result<HashMap<String, String>, ServerFnError> {
    use crate::model::load_settings;

    let settings = load_settings();
    let git_folder = settings.git_folder;
    let mut users = HashSet::new();
    if !settings.email.is_empty() {
        users.insert(settings.email.clone());
    }

    let commits = fetch_git_commit_data(&git_folder, start, end)
        .map_err(|e| ServerFnError::new(e.to_string()))?;

    let known: HashSet<_> = known_keys.iter().map(|s| s.as_str()).collect();
    let mut new_items: HashMap<String, String> = HashMap::new();

    for commit in commits {
        if users.contains(&commit.name) || users.contains(&commit.email) {
            if let Some(key) = extract_work_item_key(&commit.message) {
                if !known.contains(key.as_str()) && !new_items.contains_key(&key) {
                    new_items.insert(key, commit.message.clone());
                }
            }
        }
    }
    Ok(new_items)
}
