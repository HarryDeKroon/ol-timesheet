#![cfg(feature = "ssr")]

use base64::Engine;
use serde::Deserialize;
use serde_json::Value;
use std::collections::HashMap;
use std::sync::{LazyLock, Mutex};

static HTTP: LazyLock<reqwest::Client> = LazyLock::new(reqwest::Client::new);
static JENKINS_CONFIG: LazyLock<Option<JenkinsConfig>> = LazyLock::new(JenkinsConfig::from_env);

static REPO_JOB_CACHE: LazyLock<Mutex<HashMap<String, Option<String>>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));
static COMMIT_URL_CACHE: LazyLock<Mutex<HashMap<String, Option<String>>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

#[derive(Clone, Debug)]
struct JenkinsConfig {
    root_job_url: String,
    auth_header: Option<String>,
}

impl JenkinsConfig {
    fn from_env() -> Option<Self> {
        let base_url = std::env::var("JENKINS_BASE_URL").ok()?;
        let base_url = base_url.trim().trim_end_matches('/').to_string();
        if base_url.is_empty() {
            return None;
        }
        let root_path = std::env::var("JENKINS_ROOT_PATH")
            .unwrap_or_else(|_| "/job/ObjectifLune/job".to_string());
        let root_path = root_path.trim().trim_matches('/');
        if root_path.is_empty() {
            return None;
        }
        let root_job_url = format!("{}/{}", base_url, root_path);

        let auth_header = match (
            std::env::var("JENKINS_API_USER").ok(),
            std::env::var("JENKINS_API_TOKEN").ok(),
        ) {
            (Some(user), Some(token)) if !user.trim().is_empty() && !token.trim().is_empty() => {
                let encoded = base64::engine::general_purpose::STANDARD.encode(format!(
                    "{}:{}",
                    user.trim(),
                    token.trim()
                ));
                Some(format!("Basic {}", encoded))
            }
            _ => None,
        };

        Some(Self {
            root_job_url,
            auth_header,
        })
    }
}

#[derive(Debug, Deserialize)]
struct JenkinsJobList {
    #[serde(default)]
    jobs: Vec<JenkinsJobRef>,
}

#[derive(Clone, Debug, Deserialize)]
struct JenkinsJobRef {
    #[serde(default)]
    name: String,
    #[serde(default)]
    url: String,
}

#[derive(Debug, Deserialize)]
struct JenkinsBuildList {
    #[serde(default)]
    builds: Vec<JenkinsBuildRef>,
}

#[derive(Clone, Debug, Deserialize)]
struct JenkinsBuildRef {
    #[serde(default)]
    url: String,
    #[serde(default)]
    result: Option<String>,
}

async fn get_json<T: serde::de::DeserializeOwned>(
    config: &JenkinsConfig,
    url: &str,
) -> Result<T, String> {
    let mut request = HTTP.get(url);
    if let Some(auth) = &config.auth_header {
        request = request.header("Authorization", auth);
    }
    let response = request
        .send()
        .await
        .map_err(|e| format!("Jenkins request failed: {}", e))?;
    let status = response.status();
    if !status.is_success() {
        return Err(format!("Jenkins request {} returned {}", url, status));
    }
    response
        .json::<T>()
        .await
        .map_err(|e| format!("Jenkins JSON decode failed for {}: {}", url, e))
}

async fn url_exists(config: &JenkinsConfig, url: &str) -> Result<bool, String> {
    let mut request = HTTP.head(url);
    if let Some(auth) = &config.auth_header {
        request = request.header("Authorization", auth);
    }
    let response = request
        .send()
        .await
        .map_err(|e| format!("Jenkins request failed: {}", e))?;
    Ok(response.status().is_success())
}

fn lower(value: &str) -> String {
    value.trim().to_lowercase()
}

fn is_hex_like(value: &str) -> bool {
    let len = value.len();
    (7..=64).contains(&len) && value.chars().all(|c| c.is_ascii_hexdigit())
}

fn hash_matches(candidate: &str, commit_hash: &str) -> bool {
    if !is_hex_like(candidate) {
        return false;
    }
    let candidate_l = lower(candidate);
    let commit_l = lower(commit_hash);
    candidate_l == commit_l
        || candidate_l.starts_with(&commit_l)
        || commit_l.starts_with(&candidate_l)
}

fn value_contains_hash(value: &Value, commit_hash: &str) -> bool {
    match value {
        Value::String(s) => hash_matches(s, commit_hash),
        Value::Array(items) => items
            .iter()
            .any(|item| value_contains_hash(item, commit_hash)),
        Value::Object(map) => map
            .values()
            .any(|item| value_contains_hash(item, commit_hash)),
        _ => false,
    }
}

async fn resolve_repo_job_url(
    config: &JenkinsConfig,
    repo_slug: &str,
) -> Result<Option<String>, String> {
    let repo_key = lower(repo_slug);
    if let Ok(cache) = REPO_JOB_CACHE.lock() {
        if let Some(cached) = cache.get(&repo_key) {
            return Ok(cached.clone());
        }
    }

    let root_jobs_url = format!(
        "{}/api/json?tree=jobs[name,url]",
        config.root_job_url.trim_end_matches('/')
    );
    let root = get_json::<JenkinsJobList>(config, &root_jobs_url).await?;

    for product_job in root.jobs {
        if product_job.url.trim().is_empty() {
            continue;
        }
        let products_folder = format!("{}/job/Products", product_job.url.trim_end_matches('/'));
        let products_url = format!("{}/api/json?tree=jobs[name,url]", products_folder);
        let products = match get_json::<JenkinsJobList>(config, &products_url).await {
            Ok(result) => result,
            Err(_) => continue,
        };
        if let Some(found) = products
            .jobs
            .into_iter()
            .find(|job| lower(&job.name) == repo_key)
            .map(|job| job.url)
        {
            let value = Some(found);
            if let Ok(mut cache) = REPO_JOB_CACHE.lock() {
                cache.insert(repo_key, value.clone());
            }
            return Ok(value);
        }
    }

    if let Ok(mut cache) = REPO_JOB_CACHE.lock() {
        cache.insert(repo_key, None);
    }
    Ok(None)
}

fn pick_branch_candidates(branches: Vec<JenkinsJobRef>, issue_key: &str) -> Vec<JenkinsJobRef> {
    if branches.is_empty() {
        return branches;
    }
    let issue_upper = issue_key.trim().to_uppercase();
    if issue_upper.is_empty() {
        return branches.into_iter().take(16).collect();
    }
    let filtered = branches
        .iter()
        .filter(|job| job.name.to_uppercase().contains(&issue_upper))
        .cloned()
        .collect::<Vec<_>>();
    if filtered.is_empty() {
        branches.into_iter().take(16).collect()
    } else {
        filtered.into_iter().take(16).collect()
    }
}

async fn find_matching_build_url(
    config: &JenkinsConfig,
    branch_job_url: &str,
    commit_hash: &str,
) -> Result<Option<String>, String> {
    let builds_url = format!(
        "{}/api/json?tree=builds[url,result]",
        branch_job_url.trim_end_matches('/')
    );
    let build_list = get_json::<JenkinsBuildList>(config, &builds_url).await?;
    for build in build_list.builds.into_iter().take(20) {
        if build.url.trim().is_empty() || build.result.is_none() {
            continue;
        }
        let details_url = format!(
            "{}/api/json?tree=url,result,actions[*]",
            build.url.trim_end_matches('/')
        );
        let details = get_json::<Value>(config, &details_url).await?;
        if !value_contains_hash(&details, commit_hash) {
            continue;
        }
        let test_report_url = format!("{}/testReport", build.url.trim_end_matches('/'));
        if url_exists(config, &test_report_url).await.unwrap_or(false) {
            return Ok(Some(test_report_url));
        }
        return Ok(Some(build.url));
    }
    Ok(None)
}

pub async fn find_test_results_url_for_commit(
    repo_slug: &str,
    issue_key: &str,
    commit_hash: &str,
) -> Result<Option<String>, String> {
    let Some(config) = JENKINS_CONFIG.as_ref() else {
        return Ok(None);
    };
    if repo_slug.trim().is_empty() || commit_hash.trim().is_empty() {
        return Ok(None);
    }

    let cache_key = format!("{}|{}", lower(repo_slug), lower(commit_hash));
    if let Ok(cache) = COMMIT_URL_CACHE.lock() {
        if let Some(cached) = cache.get(&cache_key) {
            return Ok(cached.clone());
        }
    }

    let repo_job_url = match resolve_repo_job_url(config, repo_slug).await? {
        Some(url) => url,
        None => {
            if let Ok(mut cache) = COMMIT_URL_CACHE.lock() {
                cache.insert(cache_key, None);
            }
            return Ok(None);
        }
    };

    let branch_jobs_url = format!(
        "{}/api/json?tree=jobs[name,url]",
        repo_job_url.trim_end_matches('/')
    );
    let repo_job_children = get_json::<JenkinsJobList>(config, &branch_jobs_url).await?;

    let mut candidates = if repo_job_children.jobs.is_empty() {
        vec![JenkinsJobRef {
            name: repo_slug.to_string(),
            url: repo_job_url.clone(),
        }]
    } else {
        pick_branch_candidates(repo_job_children.jobs, issue_key)
    };
    if candidates.is_empty() {
        candidates.push(JenkinsJobRef {
            name: repo_slug.to_string(),
            url: repo_job_url.clone(),
        });
    }

    for branch_job in candidates {
        if branch_job.url.trim().is_empty() {
            continue;
        }
        if let Some(url) = find_matching_build_url(config, &branch_job.url, commit_hash).await? {
            if let Ok(mut cache) = COMMIT_URL_CACHE.lock() {
                cache.insert(cache_key, Some(url.clone()));
            }
            return Ok(Some(url));
        }
    }

    if let Ok(mut cache) = COMMIT_URL_CACHE.lock() {
        cache.insert(cache_key, None);
    }
    Ok(None)
}
