#![cfg(feature = "ssr")]

use crate::api::cache;
use crate::model::{Settings, WorkItem, WorklogEntry};
use base64::Engine;
use chrono::{Datelike, NaiveDate};
use serde::Deserialize;
use std::sync::LazyLock;

use log;

static HTTP: LazyLock<reqwest::Client> = LazyLock::new(reqwest::Client::new);

const JIRA_BASE: &str = "https://uplandsoftware.atlassian.net/rest/api/3";

// ─── Jira API response types ────────────────────────────────────────────────

/// Response from the new /search/jql endpoint (cursor-based pagination).
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct SearchResponse {
    issues: Vec<JiraIssue>,
    #[serde(default)]
    is_last: Option<bool>,
    next_page_token: Option<String>,
}

#[derive(Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
struct JiraAuthor {
    email_address: Option<String>,
}

#[derive(Deserialize)]
struct JiraIssue {
    key: String,
    fields: IssueFields,
}

#[derive(Deserialize)]
struct IssueFields {
    summary: String,
    issuetype: IssueType,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct IssueType {
    name: String,
    icon_url: String,
}

#[derive(Deserialize, Debug)]
struct WorklogResponse {
    worklogs: Vec<JiraWorklog>,
}

#[derive(Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
struct JiraWorklog {
    id: String,
    author: JiraAuthor,
    started: String,
    time_spent_seconds: u64,
    comment: Option<JiraComment>,
}

/// Jira v3 may return comment as a plain string (from /search/jql)
/// or as an ADF document object (from /issue/{key}/worklog).
#[derive(Deserialize, Debug)]
#[serde(untagged)]
enum JiraComment {
    Plain(String),
    Adf(serde_json::Value),
}

/// Plain text + HTML extracted from a Jira comment.
struct CommentRendered {
    plain: String,
    html: String,
    /// Raw ADF JSON string, kept for round-tripping unchanged comments.
    raw_adf: Option<String>,
}

impl JiraComment {
    fn render(&self) -> CommentRendered {
        match self {
            JiraComment::Plain(s) => CommentRendered {
                plain: s.clone(),
                html: String::new(),
                raw_adf: None,
            },
            JiraComment::Adf(v) => {
                // Improved detection: if the ADF is a single paragraph with only text children, treat as plain
                let is_plain = || {
                    if v.get("type") == Some(&serde_json::Value::String("doc".to_string()))
                        && v.get("content")
                            .and_then(|c| c.as_array())
                            .map_or(false, |arr| arr.len() == 1)
                    {
                        let para = &v["content"][0];
                        if para.get("type")
                            == Some(&serde_json::Value::String("paragraph".to_string()))
                        {
                            if let Some(children) = para.get("content").and_then(|c| c.as_array()) {
                                return children.iter().all(|child| {
                                    child.get("type")
                                        == Some(&serde_json::Value::String("text".to_string()))
                                });
                            }
                        }
                    }
                    false
                };

                if is_plain() {
                    // Concatenate all text children
                    let para = &v["content"][0];
                    let children_vec = para
                        .get("content")
                        .and_then(|c| c.as_array())
                        .cloned()
                        .unwrap_or_default();
                    let plain = children_vec
                        .iter()
                        .filter_map(|child| child.get("text").and_then(|t| t.as_str()))
                        .collect::<Vec<_>>()
                        .join("");
                    let raw_adf = serde_json::to_string(v).ok();
                    CommentRendered {
                        plain,
                        html: String::new(),
                        raw_adf,
                    }
                } else {
                    let plain = adf_extract_text(v);
                    let html = adf_to_html(v);
                    let raw_adf = serde_json::to_string(v).ok();
                    // If the HTML is just a trivial <p>…</p> wrapping the same
                    // text, skip it so the UI doesn't show a redundant preview.
                    let trivial = format!("<p>{}</p>", html_escape(&plain));
                    CommentRendered {
                        plain,
                        html: if html.trim() == trivial.trim() {
                            String::new()
                        } else {
                            html
                        },
                        raw_adf,
                    }
                }
            }
        }
    }
}

// ─── ADF → plain text ──────────────────────────────────────────────────────

/// Recursively extract structured plain text from an ADF node.
/// Paragraphs are separated by newlines, list items get bullet markers, etc.
fn adf_extract_text(v: &serde_json::Value) -> String {
    let node_type = v.get("type").and_then(|t| t.as_str()).unwrap_or("");
    match node_type {
        "text" => v
            .get("text")
            .and_then(|t| t.as_str())
            .unwrap_or("")
            .to_string(),
        "hardBreak" => "\n".to_string(),
        "paragraph" => {
            let inner = adf_extract_children_text(v);
            if inner.is_empty() {
                "\n".to_string()
            } else {
                format!("{inner}\n")
            }
        }
        "heading" => format!("{}\n", adf_extract_children_text(v)),
        "bulletList" | "orderedList" => adf_extract_children_text(v),
        "listItem" => format!("• {}\n", adf_extract_children_text(v).trim()),
        "codeBlock" => format!("{}\n", adf_extract_children_text(v)),
        "blockquote" => {
            let inner = adf_extract_children_text(v);
            inner
                .lines()
                .map(|l| format!("> {l}"))
                .collect::<Vec<_>>()
                .join("\n")
                + "\n"
        }
        "rule" => "---\n".to_string(),
        "mention" => v
            .get("attrs")
            .and_then(|a| a.get("text"))
            .and_then(|t| t.as_str())
            .unwrap_or("@unknown")
            .to_string(),
        "emoji" => v
            .get("attrs")
            .and_then(|a| a.get("text").or(a.get("shortName")))
            .and_then(|t| t.as_str())
            .unwrap_or("")
            .to_string(),
        "inlineCard" => v
            .get("attrs")
            .and_then(|a| a.get("url"))
            .and_then(|u| u.as_str())
            .unwrap_or("")
            .to_string(),
        "status" => v
            .get("attrs")
            .and_then(|a| a.get("text"))
            .and_then(|t| t.as_str())
            .map(|t| format!("[{t}]"))
            .unwrap_or_default(),
        "mediaGroup" | "mediaSingle" | "media" => String::new(),
        _ => adf_extract_children_text(v),
    }
}

fn adf_extract_children_text(v: &serde_json::Value) -> String {
    v.get("content")
        .and_then(|c| c.as_array())
        .map(|arr| {
            arr.iter()
                .map(adf_extract_text)
                .collect::<Vec<_>>()
                .join("")
        })
        .unwrap_or_default()
}

// ─── ADF → HTML ─────────────────────────────────────────────────────────────

/// Convert an ADF document to HTML, handling all common node types and marks.
fn adf_to_html(v: &serde_json::Value) -> String {
    let node_type = v.get("type").and_then(|t| t.as_str()).unwrap_or("");
    match node_type {
        "doc" => adf_render_children(v),
        "paragraph" => format!("<p>{}</p>", adf_render_children(v)),
        "heading" => {
            let level = v
                .get("attrs")
                .and_then(|a| a.get("level"))
                .and_then(|l| l.as_u64())
                .unwrap_or(1)
                .min(6);
            format!("<h{level}>{}</h{level}>", adf_render_children(v))
        }
        "text" => {
            let raw = v.get("text").and_then(|t| t.as_str()).unwrap_or("");
            let escaped = html_escape(raw);
            adf_apply_marks(&escaped, v.get("marks"))
        }
        "hardBreak" => "<br/>".to_string(),
        "bulletList" => format!("<ul>{}</ul>", adf_render_children(v)),
        "orderedList" => {
            let start = v
                .get("attrs")
                .and_then(|a| a.get("order"))
                .and_then(|o| o.as_u64())
                .unwrap_or(1);
            if start == 1 {
                format!("<ol>{}</ol>", adf_render_children(v))
            } else {
                format!(r#"<ol start="{start}">{}</ol>"#, adf_render_children(v))
            }
        }
        "listItem" => format!("<li>{}</li>", adf_render_children(v)),
        "codeBlock" => {
            let lang = v
                .get("attrs")
                .and_then(|a| a.get("language"))
                .and_then(|l| l.as_str());
            let code = adf_render_children(v);
            match lang {
                Some(l) => {
                    format!(
                        r#"<pre><code class="language-{}">{code}</code></pre>"#,
                        html_escape(l)
                    )
                }
                None => format!("<pre><code>{code}</code></pre>"),
            }
        }
        "blockquote" => format!("<blockquote>{}</blockquote>", adf_render_children(v)),
        "rule" => "<hr/>".to_string(),
        "table" => format!("<table>{}</table>", adf_render_children(v)),
        "tableRow" => format!("<tr>{}</tr>", adf_render_children(v)),
        "tableHeader" => {
            let attrs = adf_table_cell_attrs(v);
            format!("<th{attrs}>{}</th>", adf_render_children(v))
        }
        "tableCell" => {
            let attrs = adf_table_cell_attrs(v);
            format!("<td{attrs}>{}</td>", adf_render_children(v))
        }
        "mention" => {
            let text = v
                .get("attrs")
                .and_then(|a| a.get("text"))
                .and_then(|t| t.as_str())
                .unwrap_or("@unknown");
            format!(r#"<span class="adf-mention">{}</span>"#, html_escape(text))
        }
        "emoji" => {
            let fallback = v
                .get("attrs")
                .and_then(|a| a.get("text").or(a.get("shortName")))
                .and_then(|t| t.as_str())
                .unwrap_or("");
            html_escape(fallback)
        }
        "inlineCard" => {
            let url = v
                .get("attrs")
                .and_then(|a| a.get("url"))
                .and_then(|u| u.as_str())
                .unwrap_or("#");
            // Try to extract a readable label from the URL path.
            let label = url.rsplit('/').find(|s| !s.is_empty()).unwrap_or(url);
            format!(
                r#"<a href="{}" target="_blank" rel="noopener">{}</a>"#,
                html_escape(url),
                html_escape(label),
            )
        }
        "panel" => {
            let panel_type = v
                .get("attrs")
                .and_then(|a| a.get("panelType"))
                .and_then(|t| t.as_str())
                .unwrap_or("info");
            format!(
                r#"<div class="adf-panel adf-panel-{}">{}</div>"#,
                html_escape(panel_type),
                adf_render_children(v),
            )
        }
        "status" => {
            let text = v
                .get("attrs")
                .and_then(|a| a.get("text"))
                .and_then(|t| t.as_str())
                .unwrap_or("");
            let color = v
                .get("attrs")
                .and_then(|a| a.get("color"))
                .and_then(|c| c.as_str())
                .unwrap_or("neutral");
            format!(
                r#"<span class="adf-status adf-status-{}">{}</span>"#,
                html_escape(color),
                html_escape(text),
            )
        }
        "expand" => {
            let title = v
                .get("attrs")
                .and_then(|a| a.get("title"))
                .and_then(|t| t.as_str())
                .unwrap_or("");
            format!(
                "<details><summary>{}</summary>{}</details>",
                html_escape(title),
                adf_render_children(v),
            )
        }
        "date" => {
            let ts = v
                .get("attrs")
                .and_then(|a| a.get("timestamp"))
                .and_then(|t| t.as_str())
                .unwrap_or("");
            // Timestamp is milliseconds since epoch; try to format as date.
            let label = ts
                .parse::<i64>()
                .ok()
                .and_then(|ms| {
                    chrono::DateTime::from_timestamp(ms / 1000, 0)
                        .map(|dt| dt.format("%Y-%m-%d").to_string())
                })
                .unwrap_or_else(|| ts.to_string());
            format!(r#"<time>{}</time>"#, html_escape(&label))
        }
        // Media nodes require authenticated URLs; show a placeholder.
        "mediaGroup" | "mediaSingle" | "media" => {
            r#"<span class="adf-media">[media]</span>"#.to_string()
        }
        // Unknown node types: render children as a best-effort fallback.
        _ => adf_render_children(v),
    }
}

/// Render the `content` array of an ADF node.
fn adf_render_children(v: &serde_json::Value) -> String {
    v.get("content")
        .and_then(|c| c.as_array())
        .map(|arr| arr.iter().map(adf_to_html).collect::<Vec<_>>().join(""))
        .unwrap_or_default()
}

/// Build an HTML attribute string for `colspan`/`rowspan` on table cells.
fn adf_table_cell_attrs(v: &serde_json::Value) -> String {
    let attrs = v.get("attrs");
    let mut s = String::new();
    if let Some(cs) = attrs
        .and_then(|a| a.get("colspan"))
        .and_then(|n| n.as_u64())
    {
        if cs > 1 {
            s.push_str(&format!(r#" colspan="{cs}""#));
        }
    }
    if let Some(rs) = attrs
        .and_then(|a| a.get("rowspan"))
        .and_then(|n| n.as_u64())
    {
        if rs > 1 {
            s.push_str(&format!(r#" rowspan="{rs}""#));
        }
    }
    s
}

/// Wrap text in HTML tags according to ADF `marks`.
fn adf_apply_marks(text: &str, marks: Option<&serde_json::Value>) -> String {
    let Some(marks) = marks.and_then(|m| m.as_array()) else {
        return text.to_string();
    };
    let mut result = text.to_string();
    for mark in marks {
        let mark_type = mark.get("type").and_then(|t| t.as_str()).unwrap_or("");
        result = match mark_type {
            "strong" => format!("<strong>{result}</strong>"),
            "em" => format!("<em>{result}</em>"),
            "underline" => format!("<u>{result}</u>"),
            "strike" => format!("<s>{result}</s>"),
            "code" => format!("<code>{result}</code>"),
            "link" => {
                let href = mark
                    .get("attrs")
                    .and_then(|a| a.get("href"))
                    .and_then(|h| h.as_str())
                    .unwrap_or("#");
                let title = mark
                    .get("attrs")
                    .and_then(|a| a.get("title"))
                    .and_then(|t| t.as_str());
                match title {
                    Some(t) => format!(
                        r#"<a href="{}" title="{}" target="_blank" rel="noopener">{result}</a>"#,
                        html_escape(href),
                        html_escape(t),
                    ),
                    None => format!(
                        r#"<a href="{}" target="_blank" rel="noopener">{result}</a>"#,
                        html_escape(href),
                    ),
                }
            }
            "textColor" => {
                let color = mark
                    .get("attrs")
                    .and_then(|a| a.get("color"))
                    .and_then(|c| c.as_str())
                    .unwrap_or("inherit");
                format!(
                    r#"<span class="adf-text-color" data-color="{}">{result}</span>"#,
                    html_escape(color)
                )
            }
            "subsup" => {
                let typ = mark
                    .get("attrs")
                    .and_then(|a| a.get("type"))
                    .and_then(|t| t.as_str())
                    .unwrap_or("sup");
                if typ == "sub" {
                    format!("<sub>{result}</sub>")
                } else {
                    format!("<sup>{result}</sup>")
                }
            }
            _ => result,
        };
    }
    result
}

/// Minimal HTML escaping for text content and attribute values.
fn html_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

fn auth_header(settings: &Settings) -> String {
    let credentials = format!("{}:{}", settings.email, settings.upland_jira_token);
    let encoded = base64::engine::general_purpose::STANDARD.encode(credentials);
    format!("Basic {}", encoded)
}

#[derive(Deserialize, Debug, Clone)]
pub struct JiraUserProfile {
    #[serde(rename = "displayName")]
    pub display_name: String,
    #[serde(rename = "avatarUrls")]
    pub avatar_urls: AvatarUrls,
}

#[derive(Deserialize, Debug, Clone)]
pub struct AvatarUrls {
    #[serde(rename = "48x48")]
    pub size_48: String,
    // Add other sizes if needed
}

pub async fn fetch_jira_user_profile(
    settings: &crate::model::Settings,
) -> Result<JiraUserProfile, String> {
    let client = reqwest::Client::new();
    let url = format!("{}/myself", JIRA_BASE);
    let resp = client
        .get(url)
        .header("Authorization", auth_header(settings))
        .header("Accept", "application/json")
        .send()
        .await
        .map_err(|e| e.to_string())?;

    let text = resp.text().await.map_err(|e| e.to_string())?;
    log::trace!("Raw Jira /myself response: {}", text);

    serde_json::from_str::<JiraUserProfile>(&text)
        .map_err(|e| format!("deserialization error: {e}"))
}

/// Wrap a plain-text string in Atlassian Document Format (ADF),
/// which is required by the v3 worklog write endpoints.
fn make_adf_comment(text: &str) -> serde_json::Value {
    serde_json::json!({
        "version": 1,
        "type": "doc",
        "content": [
            {
                "type": "paragraph",
                "content": [
                    {
                        "type": "text",
                        "text": text
                    }
                ]
            }
        ]
    })
}

// ─── Cache-key helpers ──────────────────────────────────────────────────────

/// Cache key for all worklogs of a specific issue (user-filtered, all dates).
fn worklog_cache_key(issue_key: &str) -> String {
    format!("jira_worklogs:{}", issue_key)
}

/// Prefix for all assembled TimesheetData cache entries.
pub const TIMESHEET_DATA_PREFIX: &str = "timesheet_data:";

/// Cache key for assembled TimesheetData for a specific date range.
pub fn timesheet_data_cache_key(start: NaiveDate, end: NaiveDate) -> String {
    format!("timesheet_data:{}:{}", start, end)
}

/// Invalidate all cached data related to a specific issue.
/// Called after add/update/delete worklog operations.
pub fn invalidate_worklogs_for_issue(issue_key: &str) {
    cache::remove(&worklog_cache_key(issue_key));
    // Also invalidate all assembled timesheet data since any of them
    // might include this issue.
    cache::remove_by_prefix(TIMESHEET_DATA_PREFIX);
}

// ─── Cached worklog entry (serialised into the cache) ───────────────────────

/// Intermediate type stored in the per-issue worklog cache.
/// Contains ALL worklogs by the active user (regardless of date) so that
/// navigating to a different week does not require a new Jira API call.
#[derive(serde::Serialize, serde::Deserialize)]
struct CachedWorklogs {
    entries: Vec<WorklogEntry>,
    /// Year-to-date total hours for the current user on this issue.
    #[serde(default, alias = "all_time_total")]
    ytd_total: f64,
    /// The year for which `ytd_total` was computed, so we can invalidate
    /// the cached value when the calendar year rolls over.
    #[serde(default)]
    ytd_year: i32,
}

// ─── Public API ─────────────────────────────────────────────────────────────

/// Fetch all work items the user has logged time on in the given date range,
/// plus any assigned active tickets (status "In Progress" or "Code Review").
///
/// Uses the new `/rest/api/3/search/jql` endpoint with cursor-based pagination.
pub async fn fetch_work_items(
    settings: &Settings,
    start: NaiveDate,
    end: NaiveDate,
) -> Result<Vec<WorkItem>, String> {
    // JQL for work items with worklogs in the date range
    let worklog_jql = format!(
        "worklogAuthor = \"{}\" AND worklogDate >= \"{}\" AND worklogDate <= \"{}\"",
        settings.email, start, end
    );

    // JQL for assigned active tickets
    let assigned_jql = format!(
        "assignee = \"{}\" AND status IN (\"Code Review\", \"In Progress\")",
        settings.email
    );

    log::trace!("[fetch_work_items] worklogDate >= {start} AND worklogDate <= {end}");

    // Fetch both result sets and merge/deduplicate by issue key
    let worklog_items = fetch_work_items_by_jql(settings, &worklog_jql).await?;
    let assigned_items = fetch_work_items_by_jql(settings, &assigned_jql).await?;

    let mut seen = std::collections::HashSet::new();
    let mut items: Vec<WorkItem> = Vec::new();
    for item in worklog_items.into_iter().chain(assigned_items.into_iter()) {
        if seen.insert(item.key.clone()) {
            items.push(item);
        }
    }

    Ok(items)
}

/// Helper: fetch work items matching a single JQL query, with caching and
/// cursor-based pagination.
async fn fetch_work_items_by_jql(settings: &Settings, jql: &str) -> Result<Vec<WorkItem>, String> {
    // Check cache
    let cache_key = format!("jira_search:{}", jql);
    if let Some(cached) = cache::get(&cache_key) {
        if let Ok(items) = serde_json::from_str::<Vec<WorkItem>>(&cached) {
            return Ok(items);
        }
    }

    let mut all_issues: Vec<JiraIssue> = Vec::new();
    let mut next_page_token: Option<String> = None;

    loop {
        let mut url = format!(
            "{}/search/jql?jql={}&fields=summary,issuetype&maxResults=100",
            JIRA_BASE,
            urlencoding_jql(jql)
        );
        if let Some(ref token) = next_page_token {
            url.push_str(&format!("&nextPageToken={}", token));
        }

        let resp = HTTP
            .get(&url)
            .header("Authorization", auth_header(settings))
            .header("Accept", "application/json")
            .send()
            .await
            .map_err(|e| format!("Jira request failed: {}", e))?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            return Err(format!("Jira API error {}: {}", status, body));
        }

        let resp_text = resp
            .text()
            .await
            .map_err(|e| format!("Failed to read response body: {}", e))?;

        let page: SearchResponse = serde_json::from_str(&resp_text)
            .map_err(|e| format!("Failed to parse search response: {}", e))?;
        let is_last = page.is_last.unwrap_or(true);
        next_page_token = page.next_page_token;
        all_issues.extend(page.issues);

        if is_last || next_page_token.is_none() {
            break;
        }
    }

    let items: Vec<WorkItem> = all_issues
        .into_iter()
        .map(|issue| WorkItem {
            key: issue.key,
            summary: issue.fields.summary,
            icon_url: issue.fields.issuetype.icon_url,
            issue_type: issue.fields.issuetype.name,
        })
        .collect();

    // Cache the result
    if let Ok(json) = serde_json::to_string(&items) {
        cache::put(cache_key, json);
    }

    Ok(items)
}

// ─── Issue picker response types ────────────────────────────────────────────

#[derive(Deserialize)]
struct PickerResponse {
    sections: Vec<PickerSection>,
}

#[derive(Deserialize)]
struct PickerSection {
    #[serde(default)]
    issues: Vec<PickerIssue>,
}

#[derive(Deserialize)]
struct PickerIssue {
    key: String,
}

/// Search for Jira issues by text query using the issue picker endpoint.
///
/// The picker performs fuzzy matching on **both** the issue key and summary,
/// so typing "96320" will surface every issue whose key contains that number
/// (e.g. SHARED-96320, TIM-96320) as well as issues whose summary matches.
///
/// After the picker returns candidate keys we do a single bulk JQL fetch
/// (`key in (...)`) to retrieve proper issue-type icon URLs and type names,
/// since the picker only returns a relative `img` path that browsers cannot
/// resolve.
///
/// Returns at most `max_results` items (capped at 12).
pub async fn search_issues(
    settings: &Settings,
    query: &str,
    max_results: usize,
) -> Result<Vec<WorkItem>, String> {
    let trimmed = query.trim();
    if trimmed.is_empty() {
        return Ok(vec![]);
    }

    let cap = max_results.min(12);

    // ── Step 1: fuzzy search via the issue picker ───────────────────────
    let url = format!(
        "{}/issue/picker?query={}&showSubTasks=true&showSubTaskParent=true",
        JIRA_BASE,
        urlencoding_jql(trimmed),
    );

    log::trace!("[search_issues] picker query={}", trimmed);

    let resp = HTTP
        .get(&url)
        .header("Authorization", auth_header(settings))
        .header("Accept", "application/json")
        .send()
        .await
        .map_err(|e| format!("Jira picker request failed: {}", e))?;

    if !resp.status().is_success() {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        return Err(format!("Jira picker API error {}: {}", status, body));
    }

    let resp_text = resp
        .text()
        .await
        .map_err(|e| format!("Failed to read picker response body: {}", e))?;

    let picker: PickerResponse = serde_json::from_str(&resp_text)
        .map_err(|e| format!("Failed to parse picker response: {}", e))?;

    // Collect unique keys across all sections (history, current search, …),
    // preserving the order in which the picker ranked them.
    let mut seen = std::collections::HashSet::new();
    let mut ordered_keys: Vec<String> = Vec::new();

    for section in picker.sections {
        for issue in section.issues {
            if ordered_keys.len() >= cap {
                break;
            }
            if seen.insert(issue.key.clone()) {
                ordered_keys.push(issue.key);
            }
        }
        if ordered_keys.len() >= cap {
            break;
        }
    }

    if ordered_keys.is_empty() {
        return Ok(vec![]);
    }

    // ── Step 2: bulk fetch full issue details via JQL ───────────────────
    let keys_clause = ordered_keys
        .iter()
        .map(|k| format!("\"{}\"", k))
        .collect::<Vec<_>>()
        .join(", ");
    let jql = format!("key in ({})", keys_clause);

    let jql_url = format!(
        "{}/search/jql?jql={}&fields=summary,issuetype&maxResults={}",
        JIRA_BASE,
        urlencoding_jql(&jql),
        cap,
    );

    let jql_resp = HTTP
        .get(&jql_url)
        .header("Authorization", auth_header(settings))
        .header("Accept", "application/json")
        .send()
        .await
        .map_err(|e| format!("Jira JQL enrichment request failed: {}", e))?;

    if !jql_resp.status().is_success() {
        let status = jql_resp.status();
        let body = jql_resp.text().await.unwrap_or_default();
        return Err(format!(
            "Jira JQL enrichment API error {}: {}",
            status, body
        ));
    }

    let jql_text = jql_resp
        .text()
        .await
        .map_err(|e| format!("Failed to read JQL enrichment response: {}", e))?;

    let page: SearchResponse = serde_json::from_str(&jql_text)
        .map_err(|e| format!("Failed to parse JQL enrichment response: {}", e))?;

    // Build a lookup map so we can reassemble results in picker order.
    let detail_map: std::collections::HashMap<String, WorkItem> = page
        .issues
        .into_iter()
        .map(|issue| {
            (
                issue.key.clone(),
                WorkItem {
                    key: issue.key,
                    summary: issue.fields.summary,
                    icon_url: issue.fields.issuetype.icon_url,
                    issue_type: issue.fields.issuetype.name,
                },
            )
        })
        .collect();

    // Return items in the original picker-ranked order.
    let items: Vec<WorkItem> = ordered_keys
        .into_iter()
        .filter_map(|k| detail_map.get(&k).cloned())
        .collect();

    Ok(items)
}

/// Fetch worklogs for a single issue, filtered to the given user and date range.
///
/// The per-issue worklog cache stores ALL worklogs by the active user (all dates)
/// so that navigating to a different week does not require a new round-trip to Jira.
/// The `start`/`end` filtering is applied after the cache look-up.
pub async fn fetch_worklogs(
    settings: &Settings,
    issue_key: &str,
    start: NaiveDate,
    end: NaiveDate,
) -> Result<(Vec<WorklogEntry>, f64), String> {
    let ck = worklog_cache_key(issue_key);

    // Try cache first – returns all user worklogs regardless of date range.
    if let Some(cached_json) = cache::get(&ck) {
        if let Ok(cached) = serde_json::from_str::<CachedWorklogs>(&cached_json) {
            let current_year = chrono::Local::now().date_naive().year();
            // If the cached YTD total was computed for a different year,
            // treat it as a cache miss so we re-fetch from Jira.
            if cached.ytd_year == current_year {
                let filtered: Vec<WorklogEntry> = cached
                    .entries
                    .into_iter()
                    .filter(|w| w.date >= start && w.date <= end)
                    .collect();
                return Ok((filtered, cached.ytd_total));
            }
        }
    }

    // Cache miss – fetch from Jira
    let url = format!("{}/issue/{}/worklog", JIRA_BASE, issue_key);
    let resp = HTTP
        .get(&url)
        .header("Authorization", auth_header(settings))
        .header("Accept", "application/json")
        .send()
        .await
        .map_err(|e| format!("Jira worklog request failed: {}", e))?;

    if !resp.status().is_success() {
        return Err(format!("Jira worklog API error: {}", resp.status()));
    }

    let wl_resp: WorklogResponse = resp.json().await.map_err(|e| e.to_string())?;
    let email_lower = settings.email.to_lowercase();

    // Collect all worklogs by this user (regardless of date) so we can
    // compute both the date-filtered entries and the all-time total.
    let user_worklogs: Vec<JiraWorklog> = wl_resp
        .worklogs
        .into_iter()
        .filter(|w| {
            w.author
                .email_address
                .as_ref()
                .map(|e| e.to_lowercase() == email_lower)
                .unwrap_or(false)
        })
        .collect();

    // Year-to-date total: sum only worklogs whose date falls within the
    // current calendar year.
    let current_year = chrono::Local::now().date_naive().year();
    let ytd_start = NaiveDate::from_ymd_opt(current_year, 1, 1).unwrap();

    let ytd_total: f64 = user_worklogs
        .iter()
        .filter_map(|w| {
            let date = parse_jira_datetime(&w.started)?;
            if date >= ytd_start {
                Some(w.time_spent_seconds as f64 / 3600.0)
            } else {
                None
            }
        })
        .sum();

    // Convert ALL user worklogs to domain entries (no date filtering yet).
    let all_entries: Vec<WorklogEntry> = user_worklogs
        .into_iter()
        .filter_map(|w| {
            let date = parse_jira_datetime(&w.started)?;
            Some({
                let rendered = w
                    .comment
                    .as_ref()
                    .map(|c| c.render())
                    .unwrap_or(CommentRendered {
                        plain: String::new(),
                        html: String::new(),
                        raw_adf: None,
                    });
                WorklogEntry {
                    id: w.id,
                    issue_key: issue_key.to_string(),
                    date,
                    hours: w.time_spent_seconds as f64 / 3600.0,
                    comment: rendered.plain,
                    comment_html: rendered.html,
                    comment_adf: rendered.raw_adf,
                }
            })
        })
        .collect();

    // Store ALL entries in cache (unfiltered by date) so future week
    // navigations can be served from cache.
    let to_cache = CachedWorklogs {
        entries: all_entries.clone(),
        ytd_total,
        ytd_year: current_year,
    };
    if let Ok(json) = serde_json::to_string(&to_cache) {
        cache::put(ck, json);
    }

    // Return only the date-filtered subset.
    let filtered: Vec<WorklogEntry> = all_entries
        .into_iter()
        .filter(|w| w.date >= start && w.date <= end)
        .collect();

    Ok((filtered, ytd_total))
}

/// Add a worklog entry to a Jira issue.
/// Uses ADF format for the comment as required by the v3 API.
pub async fn add_worklog(
    settings: &Settings,
    issue_key: &str,
    date: NaiveDate,
    hours: f64,
    comment: &str,
) -> Result<String, String> {
    let url = format!("{}/issue/{}/worklog", JIRA_BASE, issue_key);
    let started = format!("{}T12:00:00.000+0000", date);
    let seconds = (hours * 3600.0).round() as u64;

    let body = serde_json::json!({
        "started": started,
        "timeSpentSeconds": seconds,
        "comment": make_adf_comment(comment),
    });

    let resp = HTTP
        .post(&url)
        .header("Authorization", auth_header(settings))
        .header("Content-Type", "application/json")
        .json(&body);

    log::trace!("About to request: {resp:?}\nwith: {body:?}");

    let resp = HTTP
        .post(&url)
        .header("Authorization", auth_header(settings))
        .header("Content-Type", "application/json")
        .json(&body)
        .send()
        .await
        .map_err(|e| e.to_string())?;

    if !resp.status().is_success() {
        let body = resp.text().await.unwrap_or_default();
        let msg = format!("Failed to add worklog: {}", body);
        log::error!("{msg}");
        return Err(msg);
    }

    // Invalidate caches for this issue
    invalidate_worklogs_for_issue(issue_key);

    // Return the new worklog ID
    let val: serde_json::Value = resp.json().await.map_err(|e| e.to_string())?;
    Ok(val["id"].as_str().unwrap_or("").to_string())
}

/// Update an existing worklog entry.
/// Uses ADF format for the comment as required by the v3 API.
///
/// When `original_adf` is `Some`, it is used as-is (preserving rich
/// formatting).  When `None`, a fresh single-paragraph ADF document
/// is created from `comment`.
pub async fn update_worklog(
    settings: &Settings,
    issue_key: &str,
    worklog_id: &str,
    hours: f64,
    comment: &str,
    original_adf: Option<&str>,
) -> Result<(), String> {
    let url = format!("{}/issue/{}/worklog/{}", JIRA_BASE, issue_key, worklog_id);
    let seconds = (hours * 3600.0).round() as u64;
    let comment_value = match original_adf {
        Some(adf_json) => {
            serde_json::from_str(adf_json).unwrap_or_else(|_| make_adf_comment(comment))
        }
        None => make_adf_comment(comment),
    };
    let body = serde_json::json!({
        "timeSpentSeconds": seconds,
        "comment": comment_value,
    });

    let resp = HTTP
        .put(&url)
        .header("Authorization", auth_header(settings))
        .header("Content-Type", "application/json")
        .json(&body)
        .send()
        .await
        .map_err(|e| e.to_string())?;

    if !resp.status().is_success() {
        let body = resp.text().await.unwrap_or_default();
        return Err(format!("Failed to update worklog: {}", body));
    }

    // Invalidate caches for this issue
    invalidate_worklogs_for_issue(issue_key);

    Ok(())
}

/// Delete a worklog entry.
pub async fn delete_worklog(
    settings: &Settings,
    issue_key: &str,
    worklog_id: &str,
) -> Result<(), String> {
    let url = format!("{}/issue/{}/worklog/{}", JIRA_BASE, issue_key, worklog_id);
    let resp = HTTP
        .delete(&url)
        .header("Authorization", auth_header(settings))
        .send()
        .await
        .map_err(|e| e.to_string())?;

    if !resp.status().is_success() {
        let body = resp.text().await.unwrap_or_default();
        return Err(format!("Failed to delete worklog: {}", body));
    }

    // Invalidate caches for this issue
    invalidate_worklogs_for_issue(issue_key);

    Ok(())
}

// ─── Prefetch ───────────────────────────────────────────────────────────────

/// Fetch and cache the `TimesheetData` for an arbitrary date range.
/// Returns `true` if the range was actually fetched (cache miss),
/// `false` if it was already cached or if an error occurred.
///
/// This is the shared building block for all prefetch functions.
async fn prefetch_range(start: NaiveDate, end: NaiveDate) -> bool {
    use std::collections::HashMap;

    let cache_key = timesheet_data_cache_key(start, end);

    // Already in cache — nothing to do.
    if cache::get(&cache_key).is_some() {
        log::debug!("[prefetch] Already cached: {} .. {}", start, end);
        return false;
    }

    let settings = crate::model::load_settings();
    if settings.email.is_empty() || settings.upland_jira_token.is_empty() {
        log::warn!("[prefetch] Skipping – Jira credentials not configured");
        return false;
    }

    log::debug!("[prefetch] Warming cache for {} .. {}", start, end);

    // 1. Fetch work items (populates jira_search cache)
    let items = match fetch_work_items(&settings, start, end).await {
        Ok(items) => items,
        Err(e) => {
            log::warn!("[prefetch] fetch_work_items failed for {}: {}", start, e);
            return false;
        }
    };

    // 2. Fetch worklogs per issue (populates jira_worklogs:{key} cache)
    let mut all_worklogs = Vec::new();
    let mut ytd_hours: HashMap<String, f64> = HashMap::new();
    for item in &items {
        match fetch_worklogs(&settings, &item.key, start, end).await {
            Ok((wls, total)) => {
                all_worklogs.extend(wls);
                ytd_hours.insert(item.key.clone(), total);
            }
            Err(e) => log::warn!("[prefetch] worklogs for {} failed: {}", item.key, e),
        }
    }

    // 3. Assemble and cache the TimesheetData — sort by key only
    let mut all_items = items;
    all_items.sort_by(|a, b| {
        fn parse_jira_key(key: &str) -> (&str, u64) {
            match key.rsplit_once('-') {
                Some((prefix, num)) => (prefix, num.parse().unwrap_or(0)),
                None => (key, 0),
            }
        }
        let (ap, an) = parse_jira_key(&a.key);
        let (bp, bn) = parse_jira_key(&b.key);
        ap.cmp(bp).then_with(|| an.cmp(&bn))
    });

    let ts = crate::model::TimesheetData {
        work_items: all_items,
        worklogs: all_worklogs,
        hours_per_week: settings.hours_per_week,
        hours_per_day: settings.hours_per_day,
        ytd_hours,
        git_commits: None,
        ..Default::default()
    };

    if let Ok(json) = serde_json::to_string(&ts) {
        cache::put(cache_key, json);
        log::info!("[prefetch] Cache warmed for {} .. {}", start, end);
    }

    true
}

/// Convenience wrapper: prefetch a single week by its Monday date.
async fn prefetch_week(monday: NaiveDate) -> bool {
    prefetch_range(monday, monday + chrono::Duration::days(6)).await
}

/// Prefetch the date ranges that would be requested when the user navigates
/// one week forward or backward from `selected_monday`, given a viewport that
/// shows `num_weeks` weeks at a time.
///
/// The multi-week view computes `start = selected_monday - (num_weeks - 1)
/// weeks` and `end = selected_monday + 6 days`, so the prefetched ranges must
/// use the same formula with shifted `selected_monday` values so the cache
/// keys will match.
///
/// Designed to be called from a background `tokio::spawn` so it never blocks
/// the response to the client.
pub async fn prefetch_adjacent_weeks(selected_monday: NaiveDate, num_weeks: usize) {
    let today = chrono::Local::now().date_naive();
    let nw = num_weeks.max(1) as i64;

    // Previous: user navigates one week back.
    let prev_monday = selected_monday - chrono::Duration::weeks(1);
    let prev_start = prev_monday - chrono::Duration::weeks(nw - 1);
    let prev_end = prev_monday + chrono::Duration::days(6);
    prefetch_range(prev_start, prev_end).await;

    // Next: user navigates one week forward, but only when the next
    // selected Monday is not after today (i.e. the week has already
    // started or is the current week).
    let next_monday = selected_monday + chrono::Duration::weeks(1);
    if next_monday <= today {
        let next_start = next_monday - chrono::Duration::weeks(nw - 1);
        let next_end = next_monday + chrono::Duration::days(6);
        prefetch_range(next_start, next_end).await;
    }
}

/// Prefetch timesheet data for the current week (and its neighbours) so the
/// very first page load is served from cache instead of blocking on slow Jira
/// API calls.
///
/// Call this once from a background `tokio::spawn` during server startup.
pub async fn prefetch_current_week() {
    use crate::components::week_navigator::week_monday;

    let settings = crate::model::load_settings();
    if settings.email.is_empty() || settings.upland_jira_token.is_empty() {
        log::warn!("[prefetch] Skipping – Jira credentials not configured");
        return;
    }

    let today = chrono::Local::now().date_naive();
    let monday = week_monday(today);

    // Current week first (highest priority).
    prefetch_week(monday).await;

    // Then the adjacent weeks (startup uses num_weeks=1 since we don't
    // know the client's viewport yet).
    prefetch_adjacent_weeks(monday, 1).await;
}

// ─── Helpers ────────────────────────────────────────────────────────────────

/// Parse a Jira datetime string (e.g. "2024-01-15T09:00:00.000+0000") into a NaiveDate.
///
/// Jira may return the `started` field in UTC regardless of the user's profile
/// timezone.  A worklog recorded on Feb 10 at midnight CET could come back as
/// `"2025-02-09T23:00:00.000+0000"`.  Naively taking the first 10 characters
/// would yield Feb 9 – one day too early.
///
/// Instead we parse the full datetime with its offset and convert to the
/// server's local timezone before extracting the date.
fn parse_jira_datetime(s: &str) -> Option<NaiveDate> {
    // Try full ISO-8601 datetime with offset -> convert to local TZ
    if let Ok(dt) = chrono::DateTime::parse_from_str(s, "%Y-%m-%dT%H:%M:%S%.3f%z") {
        let local_dt = dt.with_timezone(&chrono::Local);
        return Some(local_dt.date_naive());
    }
    // Fallback: just the date portion
    let date_str = s.get(..10)?;
    NaiveDate::parse_from_str(date_str, "%Y-%m-%d").ok()
}

/// Percent-encode a JQL query for use in a URL query parameter.
fn urlencoding_jql(jql: &str) -> String {
    jql.replace('%', "%25")
        .replace(' ', "%20")
        .replace('"', "%22")
        .replace('=', "%3D")
        .replace('<', "%3C")
        .replace('>', "%3E")
        .replace('(', "%28")
        .replace(')', "%29")
        .replace('&', "%26")
        .replace('+', "%2B")
}
