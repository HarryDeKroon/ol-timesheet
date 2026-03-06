// Bitbucket API integration is DISABLED due to API deprecation.
// All code in this file is commented out and should not be used.

// #[cfg(feature = "ssr")]
//
// use crate::model::Settings;
// use base64::Engine;
// use regex::Regex;
// use serde::Deserialize;
// use std::collections::HashSet;
// use std::sync::LazyLock;
//
// static HTTP: LazyLock<reqwest::Client> = LazyLock::new(reqwest::Client::new);
//
// static JIRA_KEY_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"[A-Z][A-Z0-9]+-\d+").unwrap());
//
// // ─── Bitbucket API response types ───────────────────────────────────────────
//
// #[derive(Deserialize)]
// struct PRListResponse {
//     values: Vec<PullRequest>,
//     next: Option<String>,
// }
//
// #[derive(Deserialize)]
// struct PullRequest {
//     title: String,
//     source: PRSource,
// }
//
// #[derive(Deserialize)]
// struct PRSource {
//     branch: PRBranch,
// }
//
// #[derive(Deserialize)]
// struct PRBranch {
//     name: String,
// }
//
// fn auth_header(settings: &Settings) -> String {
//     let credentials = format!(
//         "{}:{}",
//         settings.bitbucket_username, settings.bitbucket_app_password
//     );
//     let encoded = base64::engine::general_purpose::STANDARD.encode(credentials.clone());
//     // SGFycnkgZGUgS3Jvb246QVRCQjdUc1phS3lSNW54OURLQ1NVRmQ2SHNUMjhEODE2QzQy
//     log::info!("Bitbucket Auth: {credentials} -> |{encoded}| (cf.: |SGFycnkgZGUgS3Jvb246QVRCQjdUc1phS3lSNW54OURLQ1NVRmQ2SHNUMjhEODE2QzQy|)");
//
//     format!("Basic {}", encoded)
// }
//
// // ─── Public API ─────────────────────────────────────────────────────────────
//
// /// Fetch Jira issue keys extracted from open pull requests where the user is a participant.
// pub async fn fetch_open_pr_issue_keys(settings: &Settings) -> Result<HashSet<String>, String> {
//     if settings.bitbucket_username.is_empty() || settings.bitbucket_app_password.is_empty() {
//         return Ok(HashSet::new());
//     }
//
//     let mut keys = HashSet::new();
//     let mut url = format!(
//         "https://api.bitbucket.org/2.0/pullrequests/{}?state=OPEN",
//         settings.bitbucket_username
//     );
//
//     // Paginate through all results
//     loop {
//         let resp = HTTP
//             .get(&url)
//             .header("Authorization", auth_header(settings))
//             .header("Accept", "application/json")
//             .send()
//             .await
//             .map_err(|e| format!("Bitbucket request failed: {}", e))?;
//
//         if !resp.status().is_success() {
//             log::warn!("Bitbucket API error: {}", resp.status());
//             break;
//         }
//
//         let page: PRListResponse = resp.json().await.map_err(|e| e.to_string())?;
//
//         for pr in &page.values {
//             // Extract Jira keys from title and branch name
//             for text in [&pr.title, &pr.source.branch.name] {
//                 let upper = text.to_uppercase();
//                 for m in JIRA_KEY_RE.find_iter(&upper) {
//                     keys.insert(m.as_str().to_string());
//                 }
//             }
//         }
//
//         match page.next {
//             Some(next_url) => url = next_url,
//             None => break,
//         }
//     }
//
//     Ok(keys)
// }
