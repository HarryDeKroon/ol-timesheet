#![cfg(feature = "ssr")]

use chrono::{DateTime, Datelike, Local, NaiveDate, TimeZone, Utc};
use regex::Regex;
use serde::Deserialize;
use std::collections::{HashMap, HashSet};
use std::sync::{Arc, LazyLock, Mutex};

static HTTP: LazyLock<reqwest::Client> = LazyLock::new(reqwest::Client::new);
static ISSUE_KEY_RE: LazyLock<Option<Regex>> =
    LazyLock::new(|| Regex::new(r"^([A-Za-z][A-Za-z0-9]+-\d+)\b").ok());
const RATE_LIMIT_RETRIES: u32 = 3;
const RATE_LIMIT_DEFAULT_BACKOFF_MS: u64 = 1_000;
const RATE_LIMIT_MAX_BACKOFF_MS: u64 = 30_000;
static BITBUCKET_COOLDOWN_UNTIL: LazyLock<Mutex<Option<std::time::Instant>>> =
    LazyLock::new(|| Mutex::new(None));
static BITBUCKET_WORKSPACE_PROJECTS: LazyLock<Mutex<HashMap<String, Vec<String>>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));
static BITBUCKET_ACTIVITY_CACHE: LazyLock<Mutex<HashMap<String, BitbucketActivityCacheEntry>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));
static BITBUCKET_ACTIVITY_INFLIGHT: LazyLock<
    tokio::sync::Mutex<HashMap<String, Arc<tokio::sync::Mutex<()>>>>,
> = LazyLock::new(|| tokio::sync::Mutex::new(HashMap::new()));

#[derive(Clone, Debug)]
struct BitbucketActivityCacheEntry {
    window_start: NaiveDate,
    window_end: NaiveDate,
    activity: BitbucketActivity,
}

#[derive(Clone, Debug, Default)]
pub struct BitbucketActivity {
    pub commit_messages_by_cell: HashMap<String, Vec<String>>,
    pub commit_links_by_cell: HashMap<String, Vec<String>>,
    pub pr_review_cells: HashSet<String>,
    pub pr_links_by_cell: HashMap<String, Vec<String>>,
    pub discovered_item_summaries: HashMap<String, String>,
}

#[derive(Clone, Debug)]
struct BitbucketConfig {
    api_base: String,
    workspace: String,
    /// Service account username (for app password auth) or email (for Atlassian API token auth).
    /// Before July 28, 2026: Bitbucket username + app password.
    /// From July 28, 2026: service account email + Atlassian API token.
    api_user: String,
    /// App password or Atlassian API token for the service account.
    /// See `api_user` field for auth mode determination.
    api_token: String,
}

impl BitbucketConfig {
    fn from_env() -> Result<Self, String> {
        let api_base = std::env::var("BITBUCKET_API_BASE")
            .unwrap_or_else(|_| "https://api.bitbucket.org/2.0".to_string())
            .trim_end_matches('/')
            .to_string();

        let server_url = std::env::var("BITBUCKET_SERVER_URL").ok();
        let workspace = std::env::var("BITBUCKET_WORKSPACE")
            .ok()
            .or_else(|| {
                server_url
                    .as_deref()
                    .and_then(parse_workspace_url)
                    .map(|s| s.to_string())
            })
            .ok_or_else(|| {
                "BITBUCKET_WORKSPACE is not set and no valid BITBUCKET_SERVER_URL found".to_string()
            })?;
        let api_user = std::env::var("BITBUCKET_API_USER")
            .map_err(|_| "BITBUCKET_API_USER is not set".to_string())?;
        let api_token = std::env::var("BITBUCKET_API_TOKEN")
            .map_err(|_| "BITBUCKET_API_TOKEN is not set".to_string())?;

        Ok(Self {
            api_base,
            workspace,
            api_user,
            api_token,
        })
    }
}

fn parse_workspace_url(url: &str) -> Option<&str> {
    let trimmed = url.trim();
    let without_scheme = trimmed
        .strip_prefix("https://")
        .or_else(|| trimmed.strip_prefix("http://"))
        .unwrap_or(trimmed);
    let without_host = without_scheme.strip_prefix("bitbucket.org/")?;
    let workspace = without_host.split('/').next().unwrap_or_default().trim();
    if workspace.is_empty() {
        None
    } else {
        Some(workspace)
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
struct BitbucketProject {
    key: String,
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
    #[serde(default)]
    hash: Option<String>,
    message: String,
    date: String,
    author: Option<BitbucketAuthor>,
    #[serde(default)]
    links: Option<BitbucketLinks>,
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
    #[serde(default)]
    links: Option<BitbucketLinks>,
}

#[derive(Deserialize, Default)]
struct BitbucketLinks {
    #[serde(default)]
    html: Option<BitbucketHref>,
}

#[derive(Deserialize, Default)]
struct BitbucketHref {
    #[serde(default)]
    href: Option<String>,
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

/// Build HTTP Basic authorization header for Bitbucket service account.
///
/// Supports two auth modes:
/// - **App password (before July 28, 2026):** `api_user` = Bitbucket username, `api_token` = app password
/// - **Atlassian API token (from July 28, 2026):** `api_user` = service account email, `api_token` = API token
///
/// Both modes use the same Basic auth header format: `Basic base64(user:token)`.
fn auth_header(api_user: &str, api_token: &str) -> String {
    use base64::Engine;
    let pair = format!("{}:{}", api_user, api_token);
    let encoded = base64::engine::general_purpose::STANDARD.encode(pair);
    format!("Basic {}", encoded)
}

fn parse_iso_date(value: &str) -> Option<NaiveDate> {
    DateTime::parse_from_rfc3339(value)
        // Keep date in source timestamp offset. Converting to UTC can shift
        // a late-evening local commit into next day and place it in wrong cell.
        .map(|dt| dt.date_naive())
        .ok()
}

fn parse_iso_timestamp_utc(value: &str) -> Option<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(value)
        .map(|dt| dt.with_timezone(&Utc))
        .ok()
}

fn local_day_bounds_utc(
    start: NaiveDate,
    end: NaiveDate,
) -> Option<(DateTime<Utc>, DateTime<Utc>)> {
    let start_naive = start.and_hms_opt(0, 0, 0)?;
    let end_naive = end.and_hms_nano_opt(23, 59, 59, 999_999_999)?;

    let start_local = match Local.from_local_datetime(&start_naive) {
        chrono::LocalResult::Single(dt) => dt,
        chrono::LocalResult::Ambiguous(early, _) => early,
        chrono::LocalResult::None => return None,
    };
    let end_local = match Local.from_local_datetime(&end_naive) {
        chrono::LocalResult::Single(dt) => dt,
        chrono::LocalResult::Ambiguous(_, late) => late,
        chrono::LocalResult::None => return None,
    };
    Some((
        start_local.with_timezone(&Utc),
        end_local.with_timezone(&Utc),
    ))
}

fn is_before_scan_start(
    value: &str,
    scan_start: NaiveDate,
    scan_start_utc: Option<&DateTime<Utc>>,
) -> Option<bool> {
    if let Some(start_utc) = scan_start_utc {
        return parse_iso_timestamp_utc(value).map(|ts| ts < *start_utc);
    }
    parse_iso_date(value).map(|d| d < scan_start)
}

fn recent_weeks_window() -> (NaiveDate, NaiveDate) {
    let today = chrono::Local::now().date_naive();
    let current_week_monday =
        today - chrono::Duration::days(today.weekday().num_days_from_monday() as i64);
    let window_start = current_week_monday - chrono::Duration::weeks(2);
    let window_end = current_week_monday + chrono::Duration::days(6);
    (window_start, window_end)
}

fn clamp_to_recent_weeks(start: NaiveDate, end: NaiveDate) -> Option<(NaiveDate, NaiveDate)> {
    let (window_start, window_end) = recent_weeks_window();
    let effective_start = std::cmp::max(start, window_start);
    let effective_end = std::cmp::min(end, window_end);
    (effective_start <= effective_end).then_some((effective_start, effective_end))
}

pub fn requested_range_in_recent_weeks(start: NaiveDate, end: NaiveDate) -> bool {
    clamp_to_recent_weeks(start, end).is_some()
}

fn key_in_range(cell_key: &str, start: NaiveDate, end: NaiveDate) -> bool {
    cell_key
        .rsplit_once(':')
        .and_then(|(_, date)| NaiveDate::parse_from_str(date, "%Y-%m-%d").ok())
        .is_some_and(|d| d >= start && d <= end)
}

fn issue_key_from_cell(cell_key: &str) -> Option<String> {
    cell_key.split_once(':').map(|(key, _)| key.to_string())
}

fn filter_activity_by_range(
    source: &BitbucketActivity,
    start: NaiveDate,
    end: NaiveDate,
) -> BitbucketActivity {
    let mut filtered = BitbucketActivity::default();
    let mut referenced_keys = HashSet::<String>::new();

    for (cell_key, values) in &source.commit_messages_by_cell {
        if key_in_range(cell_key, start, end) {
            filtered
                .commit_messages_by_cell
                .insert(cell_key.clone(), values.clone());
            if let Some(key) = issue_key_from_cell(cell_key) {
                referenced_keys.insert(key);
            }
        }
    }
    for (cell_key, values) in &source.commit_links_by_cell {
        if key_in_range(cell_key, start, end) {
            filtered
                .commit_links_by_cell
                .insert(cell_key.clone(), values.clone());
            if let Some(key) = issue_key_from_cell(cell_key) {
                referenced_keys.insert(key);
            }
        }
    }
    for cell_key in &source.pr_review_cells {
        if key_in_range(cell_key, start, end) {
            filtered.pr_review_cells.insert(cell_key.clone());
            if let Some(key) = issue_key_from_cell(cell_key) {
                referenced_keys.insert(key);
            }
        }
    }
    for (cell_key, values) in &source.pr_links_by_cell {
        if key_in_range(cell_key, start, end) {
            filtered
                .pr_links_by_cell
                .insert(cell_key.clone(), values.clone());
            if let Some(key) = issue_key_from_cell(cell_key) {
                referenced_keys.insert(key);
            }
        }
    }

    for (key, summary) in &source.discovered_item_summaries {
        if referenced_keys.contains(key) {
            filtered
                .discovered_item_summaries
                .insert(key.clone(), summary.clone());
        }
    }

    filtered
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

fn reviewer_matches(reviewer: &BitbucketUser, user_account_id: &str) -> bool {
    // Match by Atlassian account_id (same across Jira and Bitbucket).
    if let Some(account_id) = &reviewer.account_id {
        if account_id.eq_ignore_ascii_case(user_account_id) {
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

fn pr_matches_user(pr: &BitbucketPullRequest, user_account_id: &str) -> bool {
    let reviewer_match = pr
        .reviewers
        .iter()
        .any(|r| reviewer_matches(r, user_account_id));
    let participant_reviewer_match = pr.participants.iter().any(|p| {
        p.role
            .as_deref()
            .map(|r| r.eq_ignore_ascii_case("REVIEWER"))
            .unwrap_or(false)
            && reviewer_matches(&p.user, user_account_id)
    });
    // Some Bitbucket responses omit explicit reviewers and only expose a
    // participant role that is not AUTHOR (e.g. PARTICIPANT) for review-assigned users.
    let participant_non_author_match = pr.participants.iter().any(|p| {
        !p.role
            .as_deref()
            .map(|r| r.eq_ignore_ascii_case("AUTHOR"))
            .unwrap_or(false)
            && reviewer_matches(&p.user, user_account_id)
    });
    reviewer_match || participant_reviewer_match || participant_non_author_match
}

fn pr_identity_key(repo_slug: &str, pr: &BitbucketPullRequest) -> String {
    if let Some(id) = pr.id {
        format!("{}:{}", repo_slug, id)
    } else {
        format!(
            "{}:{}:{}",
            repo_slug,
            pr.title,
            pr.updated_on
                .as_deref()
                .or(pr.created_on.as_deref())
                .unwrap_or_default()
        )
    }
}

fn commit_link(repo_base: &str, commit: &BitbucketCommit) -> Option<String> {
    if let Some(href) = commit
        .links
        .as_ref()
        .and_then(|l| l.html.as_ref())
        .and_then(|h| h.href.clone())
    {
        return Some(href);
    }
    commit
        .hash
        .as_ref()
        .map(|hash| format!("{}/commits/{}", repo_base, hash))
}

fn pr_link(repo_base: &str, pr: &BitbucketPullRequest) -> Option<String> {
    if let Some(href) = pr
        .links
        .as_ref()
        .and_then(|l| l.html.as_ref())
        .and_then(|h| h.href.clone())
    {
        return Some(href);
    }
    pr.id
        .map(|id| format!("{}/pull-requests/{}", repo_base, id))
}

fn parse_retry_after_ms(resp: &reqwest::Response) -> Option<u64> {
    let raw = resp
        .headers()
        .get(reqwest::header::RETRY_AFTER)?
        .to_str()
        .ok()?;
    let seconds = raw.trim().parse::<u64>().ok()?;
    Some(seconds.saturating_mul(1_000))
}

fn header_u64(resp: &reqwest::Response, names: &[&str]) -> Option<u64> {
    for name in names {
        if let Some(value) = resp.headers().get(*name) {
            if let Ok(text) = value.to_str() {
                if let Ok(parsed) = text.trim().parse::<u64>() {
                    return Some(parsed);
                }
            }
        }
    }
    None
}

fn parse_rate_limit_reset_ms(resp: &reqwest::Response) -> Option<u64> {
    let reset = header_u64(
        resp,
        &["x-ratelimit-reset", "ratelimit-reset", "x-rate-limit-reset"],
    )?;
    let now_epoch = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .ok()?
        .as_secs();
    if reset <= now_epoch {
        return Some(0);
    }
    Some(reset.saturating_sub(now_epoch).saturating_mul(1_000))
}

fn rate_limit_header_backoff_ms(resp: &reqwest::Response) -> Option<u64> {
    let remaining = header_u64(resp, &["x-ratelimit-remaining", "ratelimit-remaining"]);
    if remaining == Some(0) {
        return parse_rate_limit_reset_ms(resp);
    }
    None
}

fn set_global_cooldown(ms: u64) {
    let until = std::time::Instant::now() + std::time::Duration::from_millis(ms);
    if let Ok(mut guard) = BITBUCKET_COOLDOWN_UNTIL.lock() {
        *guard = match *guard {
            Some(existing) if existing > until => Some(existing),
            _ => Some(until),
        };
    }
}

async fn wait_for_global_cooldown() {
    let wait_for = if let Ok(guard) = BITBUCKET_COOLDOWN_UNTIL.lock() {
        (*guard).and_then(|until| until.checked_duration_since(std::time::Instant::now()))
    } else {
        None
    };
    if let Some(duration) = wait_for {
        tokio::time::sleep(duration).await;
    }
}

fn is_rate_limited_error(err: &str) -> bool {
    err.contains("429 Too Many Requests")
}

async fn get_json<T: for<'de> Deserialize<'de>>(auth_value: &str, url: &str) -> Result<T, String> {
    let mut attempt = 0u32;
    loop {
        wait_for_global_cooldown().await;
        attempt = attempt.saturating_add(1);
        log::debug!("[bitbucket] GET {}", url);
        let resp = HTTP
            .get(url)
            .header("Authorization", auth_value)
            .header("Accept", "application/json")
            .send()
            .await
            .map_err(|e| format!("Bitbucket request failed: {}", e))?;

        if resp.status().is_success() {
            return resp
                .json::<T>()
                .await
                .map_err(|e| format!("Bitbucket JSON parse failed: {}", e));
        }

        let status = resp.status();
        let retry_after_ms = if status == reqwest::StatusCode::TOO_MANY_REQUESTS {
            parse_retry_after_ms(&resp)
        } else {
            None
        };
        if status == reqwest::StatusCode::TOO_MANY_REQUESTS && attempt <= RATE_LIMIT_RETRIES {
            let retry_after_ms = retry_after_ms.unwrap_or_else(|| {
                (RATE_LIMIT_DEFAULT_BACKOFF_MS.saturating_mul(1u64 << (attempt - 1)))
                    .min(RATE_LIMIT_MAX_BACKOFF_MS)
            });
            let header_backoff_ms = rate_limit_header_backoff_ms(&resp).unwrap_or(0);
            let wait_ms = retry_after_ms.max(header_backoff_ms);
            set_global_cooldown(wait_ms);
            log::warn!(
                "[bitbucket] rate limited (429), retrying in {} ms (attempt {}/{})",
                wait_ms,
                attempt,
                RATE_LIMIT_RETRIES
            );
            tokio::time::sleep(std::time::Duration::from_millis(wait_ms)).await;
            continue;
        }
        let body = resp.text().await.unwrap_or_default();
        return Err(format!("Bitbucket API error {}: {}", status, body));
    }
}

async fn fetch_repositories_for_project(
    config: &BitbucketConfig,
    auth_value: &str,
    project_key: &str,
    start: NaiveDate,
    start_utc: Option<DateTime<Utc>>,
    _end: NaiveDate,
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
            let Some(updated_raw) = repo.updated_on.as_deref() else {
                continue;
            };
            let Some(before_start) = is_before_scan_start(updated_raw, start, start_utc.as_ref())
            else {
                continue;
            };
            if before_start {
                reached_older_than_start = true;
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
        "[bitbucket] selected {} repos updated since {} for project {}",
        slugs.len(),
        start,
        project_key
    );
    Ok(slugs)
}

async fn fetch_repositories_for_workspace(
    config: &BitbucketConfig,
    auth_value: &str,
    start: NaiveDate,
    start_utc: Option<DateTime<Utc>>,
    _end: NaiveDate,
) -> Result<Vec<String>, String> {
    let mut slugs = Vec::<String>::new();
    let mut next_url = Some(format!(
        "{}/repositories/{}?pagelen=100&sort=-updated_on",
        config.api_base, config.workspace
    ));
    let mut page_count = 0usize;

    while let Some(url) = next_url {
        let page: BitbucketPage<BitbucketProjectRepo> = get_json(auth_value, &url).await?;
        page_count += 1;
        next_url = page.next.clone();
        let mut reached_older_than_start = false;
        for repo in page.values {
            let Some(updated_raw) = repo.updated_on.as_deref() else {
                continue;
            };
            let Some(before_start) = is_before_scan_start(updated_raw, start, start_utc.as_ref())
            else {
                continue;
            };
            if before_start {
                reached_older_than_start = true;
                continue;
            }
            slugs.push(repo.slug);
        }
        if reached_older_than_start {
            break;
        }
    }
    log::info!(
        "[bitbucket] selected {} repos updated since {} for workspace {} (pages={})",
        slugs.len(),
        start,
        config.workspace,
        page_count
    );
    Ok(slugs)
}

async fn discover_workspace_project_keys(
    config: &BitbucketConfig,
    auth_value: &str,
) -> Result<Vec<String>, String> {
    if let Ok(cache) = BITBUCKET_WORKSPACE_PROJECTS.lock() {
        if let Some(keys) = cache.get(&config.workspace) {
            return Ok(keys.clone());
        }
    }

    let mut keys = Vec::<String>::new();
    let mut next_url = Some(format!(
        "{}/workspaces/{}/projects?pagelen=100",
        config.api_base, config.workspace
    ));
    while let Some(url) = next_url {
        let page: BitbucketPage<BitbucketProject> = get_json(auth_value, &url).await?;
        next_url = page.next.clone();
        for project in page.values {
            if let Ok(validated) = sanitize_project_key(&project.key, &config.api_token) {
                keys.push(validated);
            }
        }
    }

    keys.sort();
    keys.dedup();

    if let Ok(mut cache) = BITBUCKET_WORKSPACE_PROJECTS.lock() {
        cache.insert(config.workspace.clone(), keys.clone());
    }

    Ok(keys)
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
    user_account_id: &str,
    display_name: &str,
    start: NaiveDate,
    end: NaiveDate,
) -> Result<BitbucketActivity, String> {
    fetch_timesheet_activity_inner(
        user_email,
        user_account_id,
        display_name,
        start,
        end,
        true,
        BitbucketScanRange::RecentWeeksWindow,
    )
    .await
}

pub async fn fetch_timesheet_activity_fresh(
    user_email: &str,
    user_account_id: &str,
    display_name: &str,
    start: NaiveDate,
    end: NaiveDate,
) -> Result<BitbucketActivity, String> {
    fetch_timesheet_activity_inner(
        user_email,
        user_account_id,
        display_name,
        start,
        end,
        false,
        BitbucketScanRange::RecentWeeksWindow,
    )
    .await
}

pub async fn fetch_timesheet_activity_fresh_requested_window(
    user_email: &str,
    user_account_id: &str,
    display_name: &str,
    start: NaiveDate,
    end: NaiveDate,
) -> Result<BitbucketActivity, String> {
    fetch_timesheet_activity_inner(
        user_email,
        user_account_id,
        display_name,
        start,
        end,
        false,
        BitbucketScanRange::RequestedWindow,
    )
    .await
}

#[derive(Copy, Clone, Eq, PartialEq)]
enum BitbucketScanRange {
    RecentWeeksWindow,
    RequestedWindow,
}

async fn fetch_timesheet_activity_inner(
    user_email: &str,
    user_account_id: &str,
    display_name: &str,
    start: NaiveDate,
    end: NaiveDate,
    use_cache: bool,
    scan_range: BitbucketScanRange,
) -> Result<BitbucketActivity, String> {
    let (requested_start, requested_end) = match scan_range {
        BitbucketScanRange::RecentWeeksWindow => {
            let Some((effective_start, effective_end)) = clamp_to_recent_weeks(start, end) else {
                log::info!(
                    "[bitbucket] skipping fetch: requested range outside recent-week window"
                );
                return Ok(BitbucketActivity::default());
            };
            (effective_start, effective_end)
        }
        BitbucketScanRange::RequestedWindow => (start, end),
    };
    let (scan_start, scan_end) = match scan_range {
        BitbucketScanRange::RecentWeeksWindow => recent_weeks_window(),
        BitbucketScanRange::RequestedWindow => (start, end),
    };
    let scan_bounds_utc = local_day_bounds_utc(scan_start, scan_end);

    let started_at = std::time::Instant::now();
    let config = BitbucketConfig::from_env()?;
    let auth_value = auth_header(&config.api_user, &config.api_token);
    let cache_key = format!(
        "{}|{}",
        config.workspace.to_lowercase(),
        user_email.trim().to_lowercase()
    );
    if use_cache {
        if let Ok(cache) = BITBUCKET_ACTIVITY_CACHE.lock() {
            if let Some(entry) = cache.get(&cache_key) {
                if entry.window_start == scan_start && entry.window_end == scan_end {
                    log::info!(
                        "[bitbucket] cache hit for workspace={} user={} requested={}..{} scan={}..{}",
                        config.workspace,
                        user_email,
                        requested_start,
                        requested_end,
                        scan_start,
                        scan_end
                    );
                    return Ok(filter_activity_by_range(
                        &entry.activity,
                        requested_start,
                        requested_end,
                    ));
                }
            }
        }
    }
    let inflight_lock = if use_cache {
        let inflight_key = format!("{}|{}|{}", cache_key, scan_start, scan_end);
        let mut inflight = BITBUCKET_ACTIVITY_INFLIGHT.lock().await;
        Some(
            inflight
                .entry(inflight_key)
                .or_insert_with(|| Arc::new(tokio::sync::Mutex::new(())))
                .clone(),
        )
    } else {
        None
    };
    let _inflight_guard = match inflight_lock.as_ref() {
        Some(lock) => Some(lock.lock().await),
        None => None,
    };
    if use_cache {
        if let Ok(cache) = BITBUCKET_ACTIVITY_CACHE.lock() {
            if let Some(entry) = cache.get(&cache_key) {
                if entry.window_start == scan_start && entry.window_end == scan_end {
                    log::info!(
                        "[bitbucket] cache hit for workspace={} user={} requested={}..{} scan={}..{}",
                        config.workspace,
                        user_email,
                        requested_start,
                        requested_end,
                        scan_start,
                        scan_end
                    );
                    return Ok(filter_activity_by_range(
                        &entry.activity,
                        requested_start,
                        requested_end,
                    ));
                }
            }
        }
    }
    let discovered_project_keys = discover_workspace_project_keys(&config, &auth_value)
        .await
        .unwrap_or_else(|err| {
            log::warn!("[bitbucket] failed to discover workspace projects: {}", err);
            Vec::new()
        });
    let mut activity = BitbucketActivity::default();
    log::info!(
        "[bitbucket] fetching activity for workspace={} projects={} requested={}..{} scan={}..{} user_email={}",
        config.workspace,
        discovered_project_keys.join(","),
        requested_start,
        requested_end,
        scan_start,
        scan_end,
        user_email
    );
    // Commits: newest first. Stop once we go past the start date.
    let mut project_repos = Vec::<String>::new();
    let mut seen_repos = HashSet::<String>::new();
    if discovered_project_keys.is_empty() {
        let repos = fetch_repositories_for_workspace(
            &config,
            &auth_value,
            scan_start,
            scan_bounds_utc
                .as_ref()
                .map(|(start_utc, _)| start_utc.clone()),
            scan_end,
        )
        .await?;
        for repo in repos {
            if seen_repos.insert(repo.clone()) {
                project_repos.push(repo);
            }
        }
    } else {
        for project_key in &discovered_project_keys {
            let repos = fetch_repositories_for_project(
                &config,
                &auth_value,
                project_key,
                scan_start,
                scan_bounds_utc
                    .as_ref()
                    .map(|(start_utc, _)| start_utc.clone()),
                scan_end,
            )
            .await?;
            for repo in repos {
                if seen_repos.insert(repo.clone()) {
                    project_repos.push(repo);
                }
            }
        }
    }
    log::info!(
        "[bitbucket] selected {} unique repos across {} project(s)",
        project_repos.len(),
        discovered_project_keys.len()
    );
    let mut commit_page_count = 0usize;
    let mut matched_commit_count = 0usize;
    let mut commit_repo_error_count = 0usize;
    let mut rate_limit_hit = false;
    for repo_slug in &project_repos {
        if rate_limit_hit {
            break;
        }
        let repo_base = format!(
            "{}/repositories/{}/{}",
            config.api_base, config.workspace, repo_slug
        );
        log::debug!("[bitbucket] scanning commits for repo={}", repo_slug);
        let mut next_url = Some(format!("{}/commits?pagelen=100", repo_base));
        while let Some(url) = next_url {
            let page: BitbucketPage<BitbucketCommit> = match get_json(&auth_value, &url).await {
                Ok(page) => page,
                Err(err) => {
                    commit_repo_error_count += 1;
                    if is_rate_limited_error(&err) {
                        rate_limit_hit = true;
                        log::warn!(
                            "[bitbucket] commits scan rate-limited for repo={}, stopping remaining Bitbucket scans for this run",
                            repo_slug
                        );
                    } else {
                        log::warn!(
                            "[bitbucket] commits scan failed for repo={} url={} err={}",
                            repo_slug,
                            url,
                            err
                        );
                    }
                    break;
                }
            };
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
                if let Some((scan_start_utc, scan_end_utc)) = scan_bounds_utc.as_ref() {
                    let Some(commit_ts) = parse_iso_timestamp_utc(&commit.date) else {
                        continue;
                    };
                    if commit_ts < *scan_start_utc {
                        reached_older_than_start = true;
                        continue;
                    }
                    if commit_ts > *scan_end_utc
                        || !author_matches(&commit.author, user_email, display_name)
                    {
                        continue;
                    }
                } else {
                    if commit_date < scan_start {
                        reached_older_than_start = true;
                        continue;
                    }
                    if commit_date > scan_end
                        || !author_matches(&commit.author, user_email, display_name)
                    {
                        continue;
                    }
                }
                if let Some(key) = extract_work_item_key(&commit.message) {
                    let cleaned = strip_key_prefix(&commit.message, &key);
                    let map_key = format!("{}:{}", key, commit_date);
                    if let Some(link) = commit_link(&repo_base, &commit) {
                        activity
                            .commit_links_by_cell
                            .entry(map_key.clone())
                            .or_default()
                            .push(link);
                    }
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
                    scan_start
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
    let mut pr_repo_error_count = 0usize;
    let mut seen_pr_identities = HashSet::<String>::new();
    for repo_slug in &project_repos {
        if rate_limit_hit {
            break;
        }
        let repo_base = format!(
            "{}/repositories/{}/{}",
            config.api_base, config.workspace, repo_slug
        );
        log::debug!("[bitbucket] scanning pullrequests for repo={}", repo_slug);
        let mut pr_next_url = Some(format!(
            "{}/pullrequests?state=OPEN&state=MERGED&pagelen=50&sort=-updated_on",
            repo_base
        ));
        while let Some(url) = pr_next_url {
            let page: BitbucketPage<BitbucketPullRequest> = match get_json(&auth_value, &url).await
            {
                Ok(page) => page,
                Err(err) => {
                    pr_repo_error_count += 1;
                    if is_rate_limited_error(&err) {
                        rate_limit_hit = true;
                        log::warn!(
                            "[bitbucket] pullrequest scan rate-limited for repo={}, stopping remaining Bitbucket scans for this run",
                            repo_slug
                        );
                    } else {
                        log::warn!(
                            "[bitbucket] pullrequest scan failed for repo={} url={} err={}",
                            repo_slug,
                            url,
                            err
                        );
                    }
                    break;
                }
            };
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
                let when_ts_utc = pr
                    .updated_on
                    .as_deref()
                    .or(pr.created_on.as_deref())
                    .and_then(parse_iso_timestamp_utc);
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

                if let Some((scan_start_utc, scan_end_utc)) = scan_bounds_utc.as_ref() {
                    let Some(pr_ts) = when_ts_utc else {
                        continue;
                    };
                    if pr_ts < *scan_start_utc {
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
                    if pr_ts > *scan_end_utc {
                        pr_after_range_count += 1;
                        log::debug!(
                            "[bitbucket] PR skipped (after range): repo={} title={:?} date={}",
                            repo_slug,
                            pr.title,
                            pr_date
                        );
                        continue;
                    }
                } else {
                    if pr_date < scan_start {
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
                    if pr_date > scan_end {
                        pr_after_range_count += 1;
                        log::debug!(
                            "[bitbucket] PR skipped (after range): repo={} title={:?} date={}",
                            repo_slug,
                            pr.title,
                            pr_date
                        );
                        continue;
                    }
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
                let mut match_found = pr_matches_user(&pr, user_account_id);

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
                                match_found = pr_matches_user(&detail, user_account_id);
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
                        "[bitbucket] PR skipped (reviewer mismatch): repo={} id={:?} title={:?} date={} target_account_id={} reviewers=[{}] participants=[{}]",
                        repo_slug,
                        pr.id,
                        pr.title,
                        pr_date,
                        user_account_id,
                        reviewer_data.0,
                        reviewer_data.1
                    );
                    continue;
                }
                if let Some(key) = extract_work_item_key(&pr.title) {
                    seen_pr_identities.insert(pr_identity_key(repo_slug, &pr));
                    let map_key = format!("{}:{}", key, pr_date);
                    activity.pr_review_cells.insert(map_key);
                    if let Some(link) = pr_link(&repo_base, &pr) {
                        activity
                            .pr_links_by_cell
                            .entry(format!("{}:{}", key, pr_date))
                            .or_default()
                            .push(link);
                    }
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
                    scan_start
                );
                break;
            }
        }
    }

    log::info!(
        "[bitbucket] done: commit_pages={}, matched_commits={}, commit_repo_errors={}, pr_pages={}, matched_prs={}, pr_repo_errors={}, pr_missing_date={}, pr_before_range={}, pr_after_range={}, pr_reviewer_mismatch={}, pr_missing_key={}, repos_scanned={}, commit_cells={}, pr_cells={}, discovered_keys={}, elapsed_ms={}",
        commit_page_count,
        matched_commit_count,
        commit_repo_error_count,
        pr_page_count,
        matched_pr_count,
        pr_repo_error_count,
        pr_missing_date_count,
        pr_before_range_count,
        pr_after_range_count,
        pr_no_reviewer_match_count,
        pr_missing_key_count,
        project_repos.len(),
        activity.commit_messages_by_cell.len(),
        activity.pr_review_cells.len(),
        activity.discovered_item_summaries.len(),
        started_at.elapsed().as_millis()
    );

    if let Ok(mut cache) = BITBUCKET_ACTIVITY_CACHE.lock() {
        cache.insert(
            cache_key,
            BitbucketActivityCacheEntry {
                window_start: scan_start,
                window_end: scan_end,
                activity: activity.clone(),
            },
        );
    }

    Ok(filter_activity_by_range(
        &activity,
        requested_start,
        requested_end,
    ))
}

// ─── Webhook registration ────────────────────────────────────────────────────

/// Events subscribed to when registering a webhook.
const WEBHOOK_EVENTS: [&str; 5] = [
    "repo:push",
    "pullrequest:created",
    "pullrequest:updated",
    "pullrequest:fulfilled",
    "pullrequest:approved",
];

#[derive(Deserialize)]
struct BitbucketHook {
    uuid: String,
    #[serde(default)]
    url: Option<String>,
    #[serde(default)]
    description: Option<String>,
}

/// Marker stored in the hook description so this machine's hooks can be
/// found and cleaned up without touching hooks owned by other developers.
fn webhook_marker() -> String {
    let host = std::env::var("COMPUTERNAME")
        .or_else(|_| std::env::var("HOSTNAME"))
        .unwrap_or_else(|_| "unknown-host".to_string());
    format!("ol-timesheet:{}", host.to_lowercase())
}

/// Drop the whole activity cache so the next refresh re-scans Bitbucket.
/// Called when a webhook event signals new commits / PR activity.
pub fn invalidate_activity_cache() {
    if let Ok(mut cache) = BITBUCKET_ACTIVITY_CACHE.lock() {
        let evicted = cache.len();
        cache.clear();
        if evicted > 0 {
            log::info!(
                "[bitbucket] activity cache invalidated ({} entr{})",
                evicted,
                if evicted == 1 { "y" } else { "ies" }
            );
        }
    }
}

async fn send_hook_request(
    auth_value: &str,
    method: reqwest::Method,
    url: &str,
    body: Option<&serde_json::Value>,
) -> Result<serde_json::Value, String> {
    wait_for_global_cooldown().await;
    let mut req = HTTP
        .request(method.clone(), url)
        .header("Authorization", auth_value)
        .header("Accept", "application/json");
    if let Some(body) = body {
        req = req.json(body);
    }
    let resp = req
        .send()
        .await
        .map_err(|e| format!("Bitbucket request failed: {}", e))?;
    let status = resp.status();
    if status == reqwest::StatusCode::NO_CONTENT {
        return Ok(serde_json::Value::Null);
    }
    let text = resp.text().await.unwrap_or_default();
    if !status.is_success() {
        return Err(format!(
            "Bitbucket API error {} for {} {}: {}",
            status, method, url, text
        ));
    }
    if text.trim().is_empty() {
        Ok(serde_json::Value::Null)
    } else {
        serde_json::from_str(&text).map_err(|e| format!("Bitbucket JSON parse failed: {}", e))
    }
}

fn hook_payload(marker: &str, target_url: &str, secret: &str) -> serde_json::Value {
    serde_json::json!({
        "description": marker,
        "url": target_url,
        "active": true,
        "secret": secret,
        "events": WEBHOOK_EVENTS,
    })
}

/// Reconcile hooks under `hooks_base` (a `.../hooks` collection URL) so
/// exactly one hook carrying this machine's marker points at `target_url`.
async fn reconcile_hooks_at(
    auth_value: &str,
    hooks_base: &str,
    marker: &str,
    target_url: &str,
    secret: &str,
) -> Result<(), String> {
    let mut ours = Vec::<BitbucketHook>::new();
    let mut next_url = Some(format!("{}?pagelen=100", hooks_base));
    while let Some(url) = next_url {
        let page: BitbucketPage<BitbucketHook> = get_json(auth_value, &url).await?;
        next_url = page.next.clone();
        ours.extend(
            page.values
                .into_iter()
                .filter(|hook| hook.description.as_deref() == Some(marker)),
        );
    }

    let payload = hook_payload(marker, target_url, secret);
    let (matching, stale): (Vec<_>, Vec<_>) = ours
        .into_iter()
        .partition(|hook| hook.url.as_deref() == Some(target_url));

    match (matching.is_empty(), stale.first()) {
        (false, _) => {} // an up-to-date hook already exists
        (true, Some(hook)) => {
            // Repoint the first stale hook instead of delete+create.
            let hook_url = format!("{}/{}", hooks_base, hook.uuid);
            send_hook_request(auth_value, reqwest::Method::PUT, &hook_url, Some(&payload)).await?;
            log::info!("[bitbucket] webhook repointed to current tunnel URL");
        }
        (true, None) => {
            send_hook_request(
                auth_value,
                reqwest::Method::POST,
                hooks_base,
                Some(&payload),
            )
            .await?;
            log::info!("[bitbucket] webhook created at {}", hooks_base);
        }
    }

    // Remove any remaining stale duplicates (skip the one repointed above).
    let skip_first_stale = matching.is_empty();
    for hook in stale.iter().skip(if skip_first_stale { 1 } else { 0 }) {
        let hook_url = format!("{}/{}", hooks_base, hook.uuid);
        if let Err(err) =
            send_hook_request(auth_value, reqwest::Method::DELETE, &hook_url, None).await
        {
            log::warn!("[bitbucket] stale webhook cleanup failed: {}", err);
        }
    }
    Ok(())
}

/// Repositories in the configured workspace where the configured Bitbucket
/// API user has admin permission — the level required to manage webhooks. Uses the
/// workspace-scoped `role=admin` filter (the user-scoped
/// `/2.0/user/permissions/repositories` endpoint was removed; CHANGE-2770).
async fn fetch_admin_repo_slugs(
    config: &BitbucketConfig,
    auth_value: &str,
) -> Result<Vec<String>, String> {
    let mut slugs = Vec::<String>::new();
    let mut next_url = Some(format!(
        "{}/repositories/{}?role=admin&pagelen=100",
        config.api_base, config.workspace
    ));
    while let Some(url) = next_url {
        let page: BitbucketPage<BitbucketProjectRepo> = get_json(auth_value, &url).await?;
        next_url = page.next.clone();
        slugs.extend(page.values.into_iter().map(|repo| repo.slug));
    }
    slugs.sort();
    slugs.dedup();
    Ok(slugs)
}

/// Register (or update) webhooks so Bitbucket pushes `repo:push` and
/// `pullrequest:*` events to `target_url`. Tries a single workspace-level
/// hook first; when the token lacks that permission, falls back to per-repo
/// hooks on the repositories where the configured API user has admin access
/// (the level Bitbucket requires for webhook management). Returns the number
/// of hook collections reconciled.
pub async fn reconcile_webhooks(target_url: &str, secret: &str) -> Result<usize, String> {
    let config = BitbucketConfig::from_env()?;
    let auth_value = auth_header(&config.api_user, &config.api_token);
    let marker = webhook_marker();

    let workspace_hooks = format!("{}/workspaces/{}/hooks", config.api_base, config.workspace);
    match reconcile_hooks_at(&auth_value, &workspace_hooks, &marker, target_url, secret).await {
        Ok(()) => return Ok(1),
        Err(err) => log::info!(
            "[bitbucket] workspace-level webhook unavailable ({}), falling back to per-repo hooks",
            err
        ),
    }

    let slugs = fetch_admin_repo_slugs(&config, &auth_value).await?;
    if slugs.is_empty() {
        return Err(format!(
            "no repositories with admin access in workspace {}; ask a workspace admin to grant \
             repo admin or register a workspace-level webhook (other repos remain covered by \
             fallback polling)",
            config.workspace
        ));
    }
    log::info!(
        "[bitbucket] registering webhooks on {} repo(s) with admin access",
        slugs.len()
    );
    let mut reconciled = 0usize;
    for slug in &slugs {
        let repo_hooks = format!(
            "{}/repositories/{}/{}/hooks",
            config.api_base, config.workspace, slug
        );
        match reconcile_hooks_at(&auth_value, &repo_hooks, &marker, target_url, secret).await {
            Ok(()) => reconciled += 1,
            Err(err) => log::warn!(
                "[bitbucket] webhook reconciliation failed for repo {}: {}",
                slug,
                err
            ),
        }
    }
    if reconciled == 0 {
        return Err(format!(
            "webhook registration failed on all {} repositories",
            slugs.len()
        ));
    }
    Ok(reconciled)
}
