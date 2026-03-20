#![cfg(feature = "ssr")]

use crate::model::{Settings, UserSession};
use axum::{
    extract::Query,
    http::{HeaderMap, HeaderValue},
    response::{IntoResponse, Redirect, Response},
};
use base64::Engine;
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::sync::{LazyLock, Mutex, OnceLock};
use std::time::{Duration, Instant};

// ─── Constants ───────────────────────────────────────────────────────────────

pub const SESSION_COOKIE: &str = "ts_sid";
const SESSION_TTL: Duration = Duration::from_secs(86400 * 30); // 30 days
const PENDING_TTL: Duration = Duration::from_secs(600); // 10 min

const ATLASSIAN_AUTH_URL: &str = "https://auth.atlassian.com/authorize";
const ATLASSIAN_TOKEN_URL: &str = "https://auth.atlassian.com/oauth/token";
const ATLASSIAN_RESOURCES_URL: &str =
    "https://api.atlassian.com/oauth/token/accessible-resources";

// ─── OAuth configuration (set once at startup) ───────────────────────────────

#[derive(Clone, Debug)]
pub struct OAuthConfig {
    pub client_id: String,
    pub client_secret: String,
    pub redirect_uri: String,
}

static OAUTH_CONFIG: OnceLock<OAuthConfig> = OnceLock::new();

/// Initialise OAuth config from environment variables.  Must be called once
/// during server startup before any request is handled.
pub fn init_oauth(config: OAuthConfig) {
    OAUTH_CONFIG
        .set(config)
        .expect("OAuth config already initialised");
}

fn oauth_config() -> &'static OAuthConfig {
    OAUTH_CONFIG
        .get()
        .expect("OAuth config not initialised — call auth::init_oauth() at startup")
}

// ─── Session store ────────────────────────────────────────────────────────────

struct SessionEntry {
    user: UserSession,
    expires_at: Instant,
}

static SESSIONS: LazyLock<Mutex<HashMap<String, SessionEntry>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

// ─── Pending OAuth state (PKCE + CSRF, lives for up to 10 min) ───────────────

struct PendingEntry {
    pkce_verifier: String,
    created_at: Instant,
}

static PENDING: LazyLock<Mutex<HashMap<String, PendingEntry>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

// ─── Per-user preferences file I/O ───────────────────────────────────────────

fn prefs_dir(account_id: &str) -> std::path::PathBuf {
    let dirs = directories::ProjectDirs::from("com", "objectiflune", "timesheet")
        .expect("Could not determine config directory");
    let dir = dirs.config_dir().join(account_id);
    std::fs::create_dir_all(&dir).ok();
    dir
}

pub fn load_user_prefs(account_id: &str) -> Settings {
    let path = prefs_dir(account_id).join("prefs.json");
    if path.exists() {
        let data = std::fs::read_to_string(&path).unwrap_or_default();
        serde_json::from_str(&data).unwrap_or_default()
    } else {
        Settings::default()
    }
}

pub fn save_user_prefs(account_id: &str, prefs: &Settings) -> Result<(), String> {
    let path = prefs_dir(account_id).join("prefs.json");
    let data = serde_json::to_string_pretty(prefs).map_err(|e| e.to_string())?;
    std::fs::write(path, data).map_err(|e| e.to_string())
}

// ─── Public API for Leptos server functions ───────────────────────────────────

/// Extract the session from the current request and return `(session_id, UserSession)`.
///
/// Automatically refreshes the OAuth access token when it is within 5 minutes
/// of expiry.  Call this at the start of every server function that needs auth.
pub async fn current_user_session(
) -> Result<(String, UserSession), leptos::prelude::ServerFnError> {
    let headers: axum::http::HeaderMap = leptos_axum::extract()
        .await
        .map_err(|_| leptos::prelude::ServerFnError::new("Failed to extract request headers"))?;

    let session_id = extract_session_id(&headers)
        .ok_or_else(|| leptos::prelude::ServerFnError::new("Not authenticated"))?;

    let user = {
        let sessions = SESSIONS
            .lock()
            .map_err(|_| leptos::prelude::ServerFnError::new("Session store unavailable"))?;
        let entry = sessions
            .get(&session_id)
            .ok_or_else(|| leptos::prelude::ServerFnError::new("Session not found"))?;
        if Instant::now() > entry.expires_at {
            return Err(leptos::prelude::ServerFnError::new("Session expired"));
        }
        entry.user.clone()
    };

    // Refresh the access token if within 5 minutes of expiry.
    let user = if chrono::Utc::now().timestamp() + 300 >= user.expires_at {
        match do_refresh_token(&user.refresh_token).await {
            Ok((new_access, new_refresh, new_expiry)) => {
                let mut updated = user.clone();
                updated.access_token = new_access;
                updated.refresh_token = new_refresh;
                updated.expires_at = new_expiry;
                if let Ok(mut sessions) = SESSIONS.lock() {
                    if let Some(entry) = sessions.get_mut(&session_id) {
                        entry.user = updated.clone();
                    }
                }
                updated
            }
            Err(e) => {
                log::warn!("[auth] Token refresh failed: {}", e);
                user
            }
        }
    } else {
        user
    };

    Ok((session_id, user))
}

/// Persist updated preferences back into the active session.
pub fn update_session_prefs(session_id: &str, prefs: Settings) {
    if let Ok(mut sessions) = SESSIONS.lock() {
        if let Some(entry) = sessions.get_mut(session_id) {
            entry.user.preferences = prefs;
        }
    }
}

// ─── Axum route handlers ──────────────────────────────────────────────────────

/// GET /auth/login — build PKCE challenge, store pending state, redirect to Atlassian.
pub async fn login_handler() -> impl IntoResponse {
    let config = oauth_config();
    let (pkce_verifier, pkce_challenge) = generate_pkce();
    let csrf = uuid::Uuid::new_v4().to_string();

    {
        let mut pending = PENDING.lock().unwrap();
        let now = Instant::now();
        pending.retain(|_, e| now.duration_since(e.created_at) < PENDING_TTL);
        pending.insert(
            csrf.clone(),
            PendingEntry {
                pkce_verifier,
                created_at: Instant::now(),
            },
        );
    }

    let scopes = "read:jira-work write:jira-work read:jira-user offline_access";
    let auth_url = format!(
        "{}?response_type=code&client_id={}&redirect_uri={}&scope={}&state={}&code_challenge={}&code_challenge_method=S256&prompt=consent",
        ATLASSIAN_AUTH_URL,
        percent_encode(&config.client_id),
        percent_encode(&config.redirect_uri),
        percent_encode(scopes),
        percent_encode(&csrf),
        percent_encode(&pkce_challenge),
    );

    Redirect::to(&auth_url)
}

#[derive(Deserialize)]
pub struct CallbackQuery {
    pub code: Option<String>,
    pub state: Option<String>,
    pub error: Option<String>,
}

/// GET /auth/callback — exchange code for tokens, create session, set cookie.
pub async fn callback_handler(Query(params): Query<CallbackQuery>) -> Response {
    if let Some(err) = params.error {
        log::warn!("[auth] OAuth callback error from Atlassian: {}", err);
        return error_redirect("oauth_error");
    }

    let code = match params.code {
        Some(c) => c,
        None => return error_redirect("missing_code"),
    };
    let state = match params.state {
        Some(s) => s,
        None => return error_redirect("missing_state"),
    };

    // Validate CSRF and retrieve PKCE verifier.
    let pkce_verifier = {
        let mut pending = PENDING.lock().unwrap();
        match pending.remove(&state) {
            Some(entry) => {
                if Instant::now().duration_since(entry.created_at) > PENDING_TTL {
                    return error_redirect("expired_state");
                }
                entry.pkce_verifier
            }
            None => return error_redirect("invalid_state"),
        }
    };

    let config = oauth_config();

    // Exchange authorization code for tokens.
    let (access_token, refresh_token, expires_in) =
        match exchange_code(&code, &pkce_verifier, config).await {
            Ok(t) => t,
            Err(e) => {
                log::error!("[auth] Token exchange failed: {}", e);
                return error_redirect("token_exchange_failed");
            }
        };
    let expires_at = chrono::Utc::now().timestamp() + expires_in;

    // Discover the user's Jira cloud instance.
    let (cloud_id, site_url) = match fetch_accessible_resources(&access_token).await {
        Ok(r) => r,
        Err(e) => {
            log::error!("[auth] Failed to fetch accessible resources: {}", e);
            return error_redirect("resources_failed");
        }
    };

    // Fetch user profile (account_id, email, display name, avatar).
    let (account_id, email, display_name, avatar_url) =
        match fetch_user_profile(&access_token, &cloud_id).await {
            Ok(p) => p,
            Err(e) => {
                log::error!("[auth] Failed to fetch user profile: {}", e);
                return error_redirect("profile_failed");
            }
        };

    let preferences = load_user_prefs(&account_id);

    let user = UserSession {
        account_id: account_id.clone(),
        email,
        display_name,
        avatar_url,
        access_token,
        refresh_token,
        expires_at,
        cloud_id,
        site_url,
        preferences,
    };

    let session_id = uuid::Uuid::new_v4().to_string();
    {
        let mut sessions = SESSIONS.lock().unwrap();
        sessions.insert(
            session_id.clone(),
            SessionEntry {
                user,
                expires_at: Instant::now() + SESSION_TTL,
            },
        );
    }

    log::info!("[auth] Session created for account_id={}", account_id);

    let cookie_value = format!(
        "{}={}; Path=/; HttpOnly; SameSite=Lax; Max-Age={}",
        SESSION_COOKIE,
        session_id,
        SESSION_TTL.as_secs()
    );
    let mut headers = HeaderMap::new();
    headers.insert(
        axum::http::header::SET_COOKIE,
        HeaderValue::from_str(&cookie_value).unwrap(),
    );

    (headers, Redirect::to("/")).into_response()
}

/// GET /auth/logout — destroy session and redirect to login page.
pub async fn logout_handler(raw_headers: axum::http::HeaderMap) -> Response {
    if let Some(sid) = extract_session_id(&raw_headers) {
        if let Ok(mut sessions) = SESSIONS.lock() {
            if let Some(entry) = sessions.remove(&sid) {
                crate::api::cache::remove_user_cache(&entry.user.account_id);
                log::info!("[auth] Logged out: {}", entry.user.account_id);
            }
        }
    }

    let clear_cookie = format!(
        "{}=; Path=/; HttpOnly; SameSite=Lax; Max-Age=0; Expires=Thu, 01 Jan 1970 00:00:00 GMT",
        SESSION_COOKIE
    );
    let mut headers = HeaderMap::new();
    headers.insert(
        axum::http::header::SET_COOKIE,
        HeaderValue::from_str(&clear_cookie).unwrap(),
    );

    (headers, Redirect::to("/auth/login")).into_response()
}

// ─── Internal helpers ─────────────────────────────────────────────────────────

fn extract_session_id(headers: &HeaderMap) -> Option<String> {
    let cookie_str = headers.get("cookie")?.to_str().ok()?;
    for part in cookie_str.split(';') {
        let part = part.trim();
        if let Some(value) = part.strip_prefix(SESSION_COOKIE).and_then(|s| s.strip_prefix('=')) {
            return Some(value.trim().to_string());
        }
    }
    None
}

fn error_redirect(reason: &str) -> Response {
    Redirect::to(&format!("/auth/login?error={}", reason)).into_response()
}

/// Generate a PKCE `(code_verifier, code_challenge)` pair using S256 method.
fn generate_pkce() -> (String, String) {
    // Two UUID v4 values give 32 bytes of cryptographically random material.
    let uuid1 = uuid::Uuid::new_v4();
    let uuid2 = uuid::Uuid::new_v4();
    let mut bytes = [0u8; 32];
    bytes[..16].copy_from_slice(uuid1.as_bytes());
    bytes[16..].copy_from_slice(uuid2.as_bytes());

    // Verifier: base64url-encoded (no padding).
    let verifier = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(bytes);

    // Challenge: SHA-256 of verifier, base64url-encoded.
    let mut hasher = Sha256::new();
    hasher.update(verifier.as_bytes());
    let hash = hasher.finalize();
    let challenge = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(hash.as_slice());

    (verifier, challenge)
}

/// RFC 3986 percent-encoding for use in URL query parameter values.
fn percent_encode(s: &str) -> String {
    let mut out = String::with_capacity(s.len() * 3);
    for byte in s.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(byte as char);
            }
            _ => {
                out.push_str(&format!("%{:02X}", byte));
            }
        }
    }
    out
}

// ─── HTTP helpers (share the global reqwest client from jira.rs) ──────────────

static HTTP: LazyLock<reqwest::Client> = LazyLock::new(reqwest::Client::new);

#[derive(Deserialize)]
struct TokenResponse {
    access_token: String,
    #[serde(default)]
    refresh_token: Option<String>,
    #[serde(default)]
    expires_in: Option<i64>,
}

async fn exchange_code(
    code: &str,
    pkce_verifier: &str,
    config: &OAuthConfig,
) -> Result<(String, String, i64), String> {
    let body = serde_json::json!({
        "grant_type": "authorization_code",
        "client_id": config.client_id,
        "client_secret": config.client_secret,
        "code": code,
        "redirect_uri": config.redirect_uri,
        "code_verifier": pkce_verifier,
    });

    let resp = HTTP
        .post(ATLASSIAN_TOKEN_URL)
        .header("Content-Type", "application/json")
        .json(&body)
        .send()
        .await
        .map_err(|e| format!("HTTP error: {}", e))?;

    if !resp.status().is_success() {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        return Err(format!("Token endpoint {}: {}", status, body));
    }

    let token: TokenResponse = resp.json().await.map_err(|e| e.to_string())?;
    let refresh = token.refresh_token.unwrap_or_default();
    let expires_in = token.expires_in.unwrap_or(3600);
    Ok((token.access_token, refresh, expires_in))
}

/// Perform a token refresh; returns `(new_access_token, new_refresh_token, new_expires_at)`.
///
/// Atlassian uses **rotating** refresh tokens — the old token is invalidated
/// immediately.  Always persist the returned refresh token.
async fn do_refresh_token(current_refresh: &str) -> Result<(String, String, i64), String> {
    let config = oauth_config();
    let body = serde_json::json!({
        "grant_type": "refresh_token",
        "client_id": config.client_id,
        "client_secret": config.client_secret,
        "refresh_token": current_refresh,
    });

    let resp = HTTP
        .post(ATLASSIAN_TOKEN_URL)
        .header("Content-Type", "application/json")
        .json(&body)
        .send()
        .await
        .map_err(|e| format!("HTTP error: {}", e))?;

    if !resp.status().is_success() {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        return Err(format!("Refresh token error {}: {}", status, body));
    }

    let token: TokenResponse = resp.json().await.map_err(|e| e.to_string())?;
    // If Atlassian doesn't return a new refresh token (rare), keep the existing one.
    let new_refresh = token
        .refresh_token
        .unwrap_or_else(|| current_refresh.to_string());
    let new_expires_at = chrono::Utc::now().timestamp() + token.expires_in.unwrap_or(3600);
    Ok((token.access_token, new_refresh, new_expires_at))
}

async fn fetch_accessible_resources(access_token: &str) -> Result<(String, String), String> {
    let resp: serde_json::Value = HTTP
        .get(ATLASSIAN_RESOURCES_URL)
        .bearer_auth(access_token)
        .header("Accept", "application/json")
        .send()
        .await
        .map_err(|e| format!("HTTP error: {}", e))?
        .json()
        .await
        .map_err(|e| e.to_string())?;

    let first = resp
        .as_array()
        .and_then(|a| a.first())
        .ok_or("No accessible Jira resources found")?;

    let cloud_id = first["id"]
        .as_str()
        .ok_or("Missing 'id' in accessible-resources")?
        .to_string();
    let site_url = first["url"]
        .as_str()
        .unwrap_or("https://uplandsoftware.atlassian.net")
        .to_string();

    Ok((cloud_id, site_url))
}

async fn fetch_user_profile(
    access_token: &str,
    cloud_id: &str,
) -> Result<(String, String, String, String), String> {
    let url = format!(
        "https://api.atlassian.com/ex/jira/{}/rest/api/3/myself",
        cloud_id
    );
    let resp: serde_json::Value = HTTP
        .get(&url)
        .bearer_auth(access_token)
        .header("Accept", "application/json")
        .send()
        .await
        .map_err(|e| format!("HTTP error: {}", e))?
        .json()
        .await
        .map_err(|e| e.to_string())?;

    let account_id = resp["accountId"].as_str().unwrap_or("").to_string();
    let email = resp["emailAddress"].as_str().unwrap_or("").to_string();
    let display_name = resp["displayName"].as_str().unwrap_or("").to_string();
    let avatar_url = resp["avatarUrls"]["48x48"].as_str().unwrap_or("").to_string();

    Ok((account_id, email, display_name, avatar_url))
}
