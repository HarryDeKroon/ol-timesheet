#![cfg(feature = "ssr")]

use chrono::{DateTime, NaiveDate, Utc};
use regex::Regex;
use serde::Deserialize;
use std::collections::{HashMap, HashSet};
use std::sync::LazyLock;

static HTTP: LazyLock<reqwest::Client> = LazyLock::new(reqwest::Client::new);
static ISSUE_KEY_RE: LazyLock<Option<Regex>> =
    LazyLock::new(|| Regex::new(r"^([A-Za-z][A-Za-z0-9]+-\d+)\b").ok());

#[derive(Clone, Debug, Default)]
pub struct BitbucketActivity {
    pub commit_messages_by_cell: HashMap<String, Vec<String>>,
    pub pr_review_cells: HashSet<String>,
    pub discovered_item_summaries: HashMap<String, String>,
}

#[derive(Clone, Debug)]
struct BitbucketConfig {
    api_base: String,
    workspace: String,
    project_keys: Vec<String>,
    api_token: String,
}

impl BitbucketConfig {
    fn from_env() -> Result<Self, String> {
        let api_base = std::env::var("BITBUCKET_API_BASE")
            .unwrap_or_else(|_| "https://api.bitbucket.org/2.0".to_string())
            .trim_end_matches('/')
            .to_string();

        let default_project_url = std::env::var("BITBUCKET_PROJECT_URL").unwrap_or_else(|_| {
            "https://bitbucket.org/uplandsoftware/workspace/projects/OLP".to_string()
        });
        let mut project_url_values = std::env::var("BITBUCKET_PROJECT_URLS")
            .ok()
            .map(|v| split_env_list(&v))
            .unwrap_or_default();
        project_url_values.push(default_project_url);

        let parsed_projects = project_url_values
            .iter()
            .filter(|value| !value.trim().is_empty())
            .map(|value| {
                parse_project_url(value).ok_or_else(|| {
                    format!(
                        "Invalid Bitbucket project URL '{}'. Expected .../workspace/projects/<KEY>.",
                        value
                    )
                })
            })
            .collect::<Result<Vec<_>, _>>()?;

        let workspace = std::env::var("BITBUCKET_WORKSPACE")
            .ok()
            .or_else(|| parsed_projects.first().map(|(ws, _)| ws.clone()))
            .ok_or_else(|| {
                "BITBUCKET_WORKSPACE is not set and no valid BITBUCKET_PROJECT_URL(S) found"
                    .to_string()
            })?;
        let api_token = std::env::var("BITBUCKET_API_TOKEN")
            .map_err(|_| "BITBUCKET_API_TOKEN is not set".to_string())?;

        let mut project_keys = Vec::<String>::new();
        for (project_workspace, project_key) in parsed_projects {
            if !project_workspace.eq_ignore_ascii_case(&workspace) {
                log::warn!(
                    "[bitbucket] project URL workspace '{}' differs from configured workspace '{}'; using configured workspace",
                    project_workspace,
                    workspace
                );
            }
            project_keys.push(sanitize_project_key(&project_key, &api_token)?);
        }

        if let Ok(project_key_raw) = std::env::var("BITBUCKET_PROJECT_KEY") {
            for key in split_env_list(&project_key_raw) {
                project_keys.push(sanitize_project_key(&key, &api_token)?);
            }
        }

        let mut seen = HashSet::<String>::new();
        project_keys.retain(|key| seen.insert(key.clone()));
        if project_keys.is_empty() {
            return Err("No Bitbucket project keys configured".to_string());
        }

        Ok(Self {
            api_base,
            workspace,
            project_keys,
            api_token,
        })
    }
}

fn split_env_list(raw: &str) -> Vec<String> {
    raw.split(|c: char| c == ',' || c == ';' || c == '\n' || c == '\r')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(ToString::to_string)
        .collect()
}

fn parse_project_url(url: &str) -> Option<(String, String)> {
    let trimmed = url.trim();
    let without_scheme = trimmed
        .strip_prefix("https://")
        .or_else(|| trimmed.strip_prefix("http://"))
        .unwrap_or(trimmed);
    let without_host = without_scheme.strip_prefix("bitbucket.org/")?;
    let parts = without_host.split('/').collect::<Vec<_>>();
    if parts.len() < 4 {
        return None;
    }
    if parts.get(1).copied() != Some("workspace") || parts.get(2).copied() != Some("projects") {
        return None;
    }
    let workspace = parts.first()?.trim().to_string();
    let project_key = parts
        .get(3)?
        .split('?')
        .next()
        .unwrap_or_default()
        .split('#')
        .next()
        .unwrap_or_default()
        .trim()
        .trim_end_matches('/')
        .to_string();
    if workspace.is_empty() || project_key.is_empty() {
        None
    } else {
        Some((workspace, project_key))
    }
}

fn sanitize_project_key(raw: &str, api_token: &str) -> Result<String, String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Err("Bitbucket project key is empty".to_string());
    }
    if let Ok(valid) = Regex::new(r"^[A-Z][A-Z0-9_]{1,49}$") {
        if valid.is_match(trimmed) {
            return Ok(trimmed.to_string());
        }
    }

    // Common misconfiguration: token accidentally concatenated after key in URL/key.
    if !api_token.is_empty() && trimmed.ends_with(api_token) {
        let candidate = trimmed.trim_end_matches(api_token).trim();
        if let Ok(valid) = Regex::new(r"^[A-Z][A-Z0-9_]{1,49}$") {
            if valid.is_match(candidate) {
                log::warn!(
                    "[bitbucket] project key appeared to include the API token suffix; recovered project key '{}'",
                    candidate
                );
                return Ok(candidate.to_string());
            }
        }
    }

    Err(format!(
        "Invalid Bitbucket project key '{}'. Expected a short key like 'OLP'.",
        trimmed
    ))
}

#[derive(Deserialize)]
struct BitbucketProjectRepo {
    slug: String,
    #[serde(default)]
    updated_on: Option<String>,
}

#[derive(Deserialize)]
struct BitbucketPage<T> {
    values: Vec<T>,
    next: Option<String>,
}

#[derive(Deserialize)]
struct BitbucketCommit {
    message: String,
    date: String,
    author: Option<BitbucketAuthor>,
}

#[derive(Deserialize)]
struct BitbucketAuthor {
    #[serde(default)]
    raw: Option<String>,
    #[serde(default)]
    user: Option<BitbucketUser>,
}

#[derive(Deserialize)]
struct BitbucketPullRequest {
    #[serde(default)]
    id: Option<u64>,
    title: String,
    #[serde(default)]
    updated_on: Option<String>,
    #[serde(default)]
    created_on: Option<String>,
    #[serde(default)]
    reviewers: Vec<BitbucketUser>,
    #[serde(default)]
    participants: Vec<BitbucketParticipant>,
}

#[derive(Deserialize)]
struct BitbucketUser {
    #[serde(default)]
    account_id: Option<String>,
    #[serde(default)]
    uuid: Option<String>,
    #[serde(default)]
    username: Option<String>,
    #[serde(default)]
    display_name: Option<String>,
    #[serde(default)]
    nickname: Option<String>,
}

#[derive(Deserialize)]
struct BitbucketParticipant {
    #[serde(default)]
    role: Option<String>,
    user: BitbucketUser,
}

#[derive(Deserialize)]
struct BitbucketCurrentUser {
    #[serde(default)]
    account_id: Option<String>,
    #[serde(default)]
    uuid: Option<String>,
    #[serde(default)]
    username: Option<String>,
    #[serde(default)]
    display_name: Option<String>,
    #[serde(default)]
    nickname: Option<String>,
}

fn auth_header(user_email: &str, api_token: &str) -> String {
    use base64::Engine;
    let pair = format!("{}:{}", user_email, api_token);
    let encoded = base64::engine::general_purpose::STANDARD.encode(pair);
    format!("Basic {}", encoded)
}

fn parse_iso_date(value: &str) -> Option<NaiveDate> {
    DateTime::parse_from_rfc3339(value)
        .map(|dt| dt.with_timezone(&Utc).date_naive())
        .ok()
}

fn extract_work_item_key(text: &str) -> Option<String> {
    let cap = ISSUE_KEY_RE.as_ref()?.captures(text)?;
    cap.get(1).map(|m| m.as_str().to_uppercase())
}

fn strip_key_prefix(text: &str, key: &str) -> String {
    let pattern = format!(r"(?i)^{}\s*[:\-\s]\s*", regex::escape(key));
    if let Ok(re) = Regex::new(&pattern) {
        let stripped = re.replace(text, "").trim().to_string();
        if stripped.is_empty() {
            text.trim().to_string()
        } else {
            stripped
        }
    } else {
        text.trim().to_string()
    }
}

fn author_matches(author: &Option<BitbucketAuthor>, user_email: &str, display_name: &str) -> bool {
    let email_l = user_email.trim().to_lowercase();
    let display_l = display_name.trim().to_lowercase();

    let Some(author) = author else {
        return false;
    };

    if let Some(raw) = &author.raw {
        let raw_l = raw.to_lowercase();
        if !email_l.is_empty() && raw_l.contains(&email_l) {
            return true;
        }
        if !display_l.is_empty() && raw_l.contains(&display_l) {
            return true;
        }
    }

    if let Some(user) = &author.user {
        if let Some(name) = &user.display_name {
            if !display_l.is_empty() && name.eq_ignore_ascii_case(&display_l) {
                return true;
            }
        }
        if let Some(nick) = &user.nickname {
            if nick.eq_ignore_ascii_case(user_email) {
                return true;
            }
            let local = user_email.split('@').next().unwrap_or_default();
            if !local.is_empty() && nick.eq_ignore_ascii_case(local) {
                return true;
            }
        }
    }

    false
}

fn reviewer_matches(reviewer: &BitbucketUser, user_email: &str, display_name: &str) -> bool {
    let email = user_email.trim();
    let display = display_name.trim();
    let local = email.split('@').next().unwrap_or_default();

    if let Some(name) = &reviewer.display_name {
        if !display.is_empty() && name.eq_ignore_ascii_case(display) {
            return true;
        }
    }
    if let Some(username) = &reviewer.username {
        if !email.is_empty() && username.eq_ignore_ascii_case(email) {
            return true;
        }
        if !local.is_empty() && username.eq_ignore_ascii_case(local) {
            return true;
        }
    }
    if let Some(nick) = &reviewer.nickname {
        if !email.is_empty() && nick.eq_ignore_ascii_case(email) {
            return true;
        }
        if !local.is_empty() && nick.eq_ignore_ascii_case(local) {
            return true;
        }
    }
    false
}

fn reviewer_matches_identity(
    reviewer: &BitbucketUser,
    current_user: &BitbucketCurrentUser,
) -> bool {
    if let (Some(a), Some(b)) = (&reviewer.account_id, &current_user.account_id) {
        if a == b {
            return true;
        }
    }
    if let (Some(a), Some(b)) = (&reviewer.uuid, &current_user.uuid) {
        if a == b {
            return true;
        }
    }
    if let (Some(a), Some(b)) = (&reviewer.username, &current_user.username) {
        if a.eq_ignore_ascii_case(b) {
            return true;
        }
    }
    if let (Some(a), Some(b)) = (&reviewer.nickname, &current_user.nickname) {
        if a.eq_ignore_ascii_case(b) {
            return true;
        }
    }
    if let (Some(a), Some(b)) = (&reviewer.display_name, &current_user.display_name) {
        if a.eq_ignore_ascii_case(b) {
            return true;
        }
    }
    false
}

fn reviewer_debug_identity(reviewer: &BitbucketUser) -> String {
    format!(
        "account_id={:?}, uuid={:?}, username={:?}, nickname={:?}, display_name={:?}",
        reviewer.account_id,
        reviewer.uuid,
        reviewer.username,
        reviewer.nickname,
        reviewer.display_name
    )
}

fn pr_matches_user(
    pr: &BitbucketPullRequest,
    current_user: Option<&BitbucketCurrentUser>,
    user_email: &str,
    display_name: &str,
) -> bool {
    let reviewer_match = pr.reviewers.iter().any(|r| {
        current_user
            .map(|u| reviewer_matches_identity(r, u))
            .unwrap_or(false)
            || reviewer_matches(r, user_email, display_name)
    });
    let participant_reviewer_match = pr.participants.iter().any(|p| {
        p.role
            .as_deref()
            .map(|r| r.eq_ignore_ascii_case("REVIEWER"))
            .unwrap_or(false)
            && (current_user
                .map(|u| reviewer_matches_identity(&p.user, u))
                .unwrap_or(false)
                || reviewer_matches(&p.user, user_email, display_name))
    });
    reviewer_match || participant_reviewer_match
}

fn current_user_debug_identity(user: &BitbucketCurrentUser) -> String {
    format!(
        "account_id={:?}, uuid={:?}, username={:?}, nickname={:?}, display_name={:?}",
        user.account_id, user.uuid, user.username, user.nickname, user.display_name
    )
}

async fn get_json<T: for<'de> Deserialize<'de>>(auth_value: &str, url: &str) -> Result<T, String> {
    log::debug!("[bitbucket] GET {}", url);
    let resp = HTTP
        .get(url)
        .header("Authorization", auth_value)
        .header("Accept", "application/json")
        .send()
        .await
        .map_err(|e| format!("Bitbucket request failed: {}", e))?;

    if !resp.status().is_success() {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        log::warn!("[bitbucket] request failed: {} {}", status, url);
        return Err(format!("Bitbucket API error {}: {}", status, body));
    }

    resp.json::<T>()
        .await
        .map_err(|e| format!("Bitbucket JSON parse failed: {}", e))
}

async fn fetch_repositories_for_project(
    config: &BitbucketConfig,
    auth_value: &str,
    project_key: &str,
    start: NaiveDate,
    end: NaiveDate,
) -> Result<Vec<String>, String> {
    let mut slugs = Vec::<String>::new();
    let mut next_url = Some(format!(
        "{}/repositories/{}?q={}&pagelen=100&sort=-updated_on",
        config.api_base,
        config.workspace,
        urlencoding_jql(&format!("project.key=\"{}\"", project_key))
    ));
    let mut page_count = 0usize;

    while let Some(url) = next_url {
        let page: BitbucketPage<BitbucketProjectRepo> = get_json(auth_value, &url).await?;
        page_count += 1;
        let page_len = page.values.len();
        next_url = page.next.clone();
        log::debug!(
            "[bitbucket] project repos page {} items={}",
            page_count,
            page_len
        );

        let mut reached_older_than_start = false;
        for repo in page.values {
            let Some(updated) = repo.updated_on.as_deref().and_then(parse_iso_date) else {
                continue;
            };
            if updated < start {
                reached_older_than_start = true;
                continue;
            }

            if updated > end {
                continue;
            }
            slugs.push(repo.slug);
        }

        if reached_older_than_start {
            log::debug!(
                "[bitbucket] repo pagination stopped after reaching repos older than {}",
                start
            );
            break;
        }
    }

    log::info!(
        "[bitbucket] selected {} repos modified in range {}..{} for project {}",
        slugs.len(),
        start,
        end,
        project_key
    );
    Ok(slugs)
}

fn urlencoding_jql(s: &str) -> String {
    s.chars()
        .map(|c| match c {
            'A'..='Z' | 'a'..='z' | '0'..='9' | '-' | '_' | '.' | '~' => c.to_string(),
            _ => format!("%{:02X}", c as u32),
        })
        .collect()
}

pub async fn fetch_timesheet_activity(
    user_email: &str,
    display_name: &str,
    start: NaiveDate,
    end: NaiveDate,
) -> Result<BitbucketActivity, String> {
    let config = BitbucketConfig::from_env()?;
    let auth_value = auth_header(user_email, &config.api_token);
    let mut activity = BitbucketActivity::default();
    log::info!(
        "[bitbucket] fetching activity for workspace={} projects={} range={}..{} user_email={}",
        config.workspace,
        config.project_keys.join(","),
        start,
        end,
        user_email
    );
    let current_user: Option<BitbucketCurrentUser> = match get_json::<BitbucketCurrentUser>(
        &auth_value,
        &format!("{}/user", config.api_base),
    )
    .await
    {
        Ok(u) => {
            log::debug!(
                "[bitbucket] current user {}",
                current_user_debug_identity(&u)
            );
            Some(u)
        }
        Err(e) => {
            log::warn!(
                "[bitbucket] failed to resolve current user via /user: {}",
                e
            );
            None
        }
    };

    // Commits: newest first. Stop once we go past the start date.
    let mut project_repos = Vec::<String>::new();
    let mut seen_repos = HashSet::<String>::new();
    for project_key in &config.project_keys {
        let repos =
            fetch_repositories_for_project(&config, &auth_value, project_key, start, end).await?;
        for repo in repos {
            if seen_repos.insert(repo.clone()) {
                project_repos.push(repo);
            }
        }
    }
    log::info!(
        "[bitbucket] selected {} unique repos across {} project(s)",
        project_repos.len(),
        config.project_keys.len()
    );
    let mut commit_page_count = 0usize;
    let mut matched_commit_count = 0usize;
    for repo_slug in &project_repos {
        let repo_base = format!(
            "{}/repositories/{}/{}",
            config.api_base, config.workspace, repo_slug
        );
        log::debug!("[bitbucket] scanning commits for repo={}", repo_slug);
        let mut next_url = Some(format!("{}/commits?pagelen=100", repo_base));
        while let Some(url) = next_url {
            let page: BitbucketPage<BitbucketCommit> = get_json(&auth_value, &url).await?;
            commit_page_count += 1;
            let page_len = page.values.len();
            next_url = page.next.clone();
            log::debug!(
                "[bitbucket] commits page {} repo={} items={}",
                commit_page_count,
                repo_slug,
                page_len
            );

            let mut reached_older_than_start = false;
            for commit in page.values {
                let Some(commit_date) = parse_iso_date(&commit.date) else {
                    continue;
                };
                if commit_date < start {
                    reached_older_than_start = true;
                    continue;
                }
                if commit_date > end || !author_matches(&commit.author, user_email, display_name) {
                    continue;
                }
                if let Some(key) = extract_work_item_key(&commit.message) {
                    let cleaned = strip_key_prefix(&commit.message, &key);
                    let map_key = format!("{}:{}", key, commit_date);
                    activity
                        .commit_messages_by_cell
                        .entry(map_key)
                        .or_default()
                        .push(cleaned.clone());
                    activity
                        .discovered_item_summaries
                        .entry(key)
                        .or_insert(cleaned);
                    matched_commit_count += 1;
                }
            }

            if reached_older_than_start {
                log::debug!(
                    "[bitbucket] commit pagination stopped for repo={} after reaching data older than {}",
                    repo_slug,
                    start
                );
                break;
            }
        }
    }

    // Pull requests where the active user is in reviewers; map to updated_on day.
    let mut pr_page_count = 0usize;
    let mut matched_pr_count = 0usize;
    let mut pr_missing_date_count = 0usize;
    let mut pr_before_range_count = 0usize;
    let mut pr_after_range_count = 0usize;
    let mut pr_no_reviewer_match_count = 0usize;
    let mut pr_missing_key_count = 0usize;
    for repo_slug in &project_repos {
        let repo_base = format!(
            "{}/repositories/{}/{}",
            config.api_base, config.workspace, repo_slug
        );
        log::debug!("[bitbucket] scanning pullrequests for repo={}", repo_slug);
        let mut pr_next_url = Some(format!(
            "{}/pullrequests?pagelen=50&sort=-updated_on",
            repo_base
        ));
        while let Some(url) = pr_next_url {
            let page: BitbucketPage<BitbucketPullRequest> = get_json(&auth_value, &url).await?;
            pr_page_count += 1;
            let page_len = page.values.len();
            pr_next_url = page.next.clone();
            log::debug!(
                "[bitbucket] pullrequests page {} repo={} items={}",
                pr_page_count,
                repo_slug,
                page_len
            );

            let mut reached_older_than_start = false;
            for pr in page.values {
                let when = pr
                    .updated_on
                    .as_deref()
                    .or(pr.created_on.as_deref())
                    .and_then(parse_iso_date);
                let Some(pr_date) = when else {
                    pr_missing_date_count += 1;
                    log::debug!(
                        "[bitbucket] PR skipped (missing parsable date): repo={} title={:?} updated_on={:?} created_on={:?}",
                        repo_slug,
                        pr.title,
                        pr.updated_on,
                        pr.created_on
                    );
                    continue;
                };

                if pr_date < start {
                    reached_older_than_start = true;
                    pr_before_range_count += 1;
                    log::debug!(
                        "[bitbucket] PR skipped (before range): repo={} title={:?} date={}",
                        repo_slug,
                        pr.title,
                        pr_date
                    );
                    continue;
                }
                if pr_date > end {
                    pr_after_range_count += 1;
                    log::debug!(
                        "[bitbucket] PR skipped (after range): repo={} title={:?} date={}",
                        repo_slug,
                        pr.title,
                        pr_date
                    );
                    continue;
                }
                let mut reviewer_data = (
                    pr.reviewers
                        .iter()
                        .map(reviewer_debug_identity)
                        .collect::<Vec<_>>()
                        .join(" | "),
                    pr.participants
                        .iter()
                        .map(|p| format!("role={:?}, {}", p.role, reviewer_debug_identity(&p.user)))
                        .collect::<Vec<_>>()
                        .join(" | "),
                );
                let mut match_found =
                    pr_matches_user(&pr, current_user.as_ref(), user_email, display_name);

                // List endpoint may omit reviewer/participant expansions in some environments.
                if !match_found
                    && pr.reviewers.is_empty()
                    && pr.participants.is_empty()
                    && pr.id.is_some()
                {
                    if let Some(pr_id) = pr.id {
                        let pr_detail_url = format!("{}/pullrequests/{}", repo_base, pr_id);
                        match get_json::<BitbucketPullRequest>(&auth_value, &pr_detail_url).await {
                            Ok(detail) => {
                                reviewer_data = (
                                    detail
                                        .reviewers
                                        .iter()
                                        .map(reviewer_debug_identity)
                                        .collect::<Vec<_>>()
                                        .join(" | "),
                                    detail
                                        .participants
                                        .iter()
                                        .map(|p| {
                                            format!(
                                                "role={:?}, {}",
                                                p.role,
                                                reviewer_debug_identity(&p.user)
                                            )
                                        })
                                        .collect::<Vec<_>>()
                                        .join(" | "),
                                );
                                match_found = pr_matches_user(
                                    &detail,
                                    current_user.as_ref(),
                                    user_email,
                                    display_name,
                                );
                                log::debug!(
                                    "[bitbucket] PR detail lookup: repo={} id={} match={} reviewers=[{}] participants=[{}]",
                                    repo_slug,
                                    pr_id,
                                    match_found,
                                    reviewer_data.0,
                                    reviewer_data.1
                                );
                            }
                            Err(err) => {
                                log::debug!(
                                    "[bitbucket] PR detail lookup failed: repo={} id={} err={}",
                                    repo_slug,
                                    pr_id,
                                    err
                                );
                            }
                        }
                    }
                }

                if !match_found {
                    pr_no_reviewer_match_count += 1;
                    log::debug!(
                        "[bitbucket] PR skipped (reviewer mismatch): repo={} id={:?} title={:?} date={} reviewers=[{}] participants=[{}]",
                        repo_slug,
                        pr.id,
                        pr.title,
                        pr_date,
                        reviewer_data.0,
                        reviewer_data.1
                    );
                    if pr_no_reviewer_match_count <= 5 {
                        let current_user_details = current_user
                            .as_ref()
                            .map(current_user_debug_identity)
                            .unwrap_or_else(|| "unavailable".to_string());
                        log::info!(
                            "[bitbucket] reviewer mismatch sample #{}: repo={} id={:?} title={:?} current_user={} reviewers=[{}] participants=[{}]",
                            pr_no_reviewer_match_count,
                            repo_slug,
                            pr.id,
                            pr.title,
                            current_user_details,
                            reviewer_data.0,
                            reviewer_data.1
                        );
                    }
                    continue;
                }
                if let Some(key) = extract_work_item_key(&pr.title) {
                    let map_key = format!("{}:{}", key, pr_date);
                    activity.pr_review_cells.insert(map_key);
                    let cleaned = strip_key_prefix(&pr.title, &key);
                    activity
                        .discovered_item_summaries
                        .entry(key.clone())
                        .or_insert(cleaned);
                    matched_pr_count += 1;
                    log::debug!(
                        "[bitbucket] PR matched: repo={} title={:?} date={} key={}",
                        repo_slug,
                        pr.title,
                        pr_date,
                        key
                    );
                } else {
                    pr_missing_key_count += 1;
                    log::debug!(
                        "[bitbucket] PR skipped (no work-item key at title start): repo={} title={:?} date={}",
                        repo_slug,
                        pr.title,
                        pr_date
                    );
                }
            }

            if reached_older_than_start {
                log::debug!(
                    "[bitbucket] pullrequest pagination stopped for repo={} after reaching data older than {}",
                    repo_slug,
                    start
                );
                break;
            }
        }
    }

    log::info!(
        "[bitbucket] done: commit_pages={}, matched_commits={}, pr_pages={}, matched_prs={}, pr_missing_date={}, pr_before_range={}, pr_after_range={}, pr_reviewer_mismatch={}, pr_missing_key={}, repos_scanned={}, commit_cells={}, pr_cells={}, discovered_keys={}",
        commit_page_count,
        matched_commit_count,
        pr_page_count,
        matched_pr_count,
        pr_missing_date_count,
        pr_before_range_count,
        pr_after_range_count,
        pr_no_reviewer_match_count,
        pr_missing_key_count,
        project_repos.len(),
        activity.commit_messages_by_cell.len(),
        activity.pr_review_cells.len(),
        activity.discovered_item_summaries.len()
    );

    Ok(activity)
}
