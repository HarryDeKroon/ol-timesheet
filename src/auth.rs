#![cfg(feature = "ssr")]

use crate::model::{Settings, UserSession};
use axum::{
    extract::Query,
    http::{HeaderMap, HeaderValue},
    response::{IntoResponse, Redirect, Response},
};
use base64::Engine;
use hmac::{Hmac, Mac};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::sync::{LazyLock, Mutex, OnceLock};
use std::time::{Duration, Instant};

type HmacSha256 = Hmac<Sha256>;

// ─── Constants ────────────────────────────────────────────────────────────────

pub const SESSION_COOKIE: &str = "ts_sid";
/// Sliding session TTL: reset to this on every authenticated request.
const SESSION_TTL_SECS: i64 = 86400 * 4; // 4 days
const SESSION_TTL: Duration = Duration::from_secs(SESSION_TTL_SECS as u64);
const PENDING_TTL: Duration = Duration::from_secs(600); // 10 min
/// Half-window for replay-nonce timestamp validation (±5 min).
const NONCE_WINDOW_SECS: i64 = 300;

const ATLASSIAN_AUTH_URL: &str = "https://auth.atlassian.com/authorize";
const ATLASSIAN_TOKEN_URL: &str = "https://auth.atlassian.com/oauth/token";
const ATLASSIAN_RESOURCES_URL: &str =
    "https://api.atlassian.com/oauth/token/accessible-resources";

// ─── App config directory ─────────────────────────────────────────────────────

fn app_config_dir() -> std::path::PathBuf {
    let dirs = directories::ProjectDirs::from("com", "objectiflune", "timesheet")
        .expect("Could not determine config directory");
    let dir = dirs.config_dir().to_path_buf();
    std::fs::create_dir_all(&dir).ok();
    dir
}

// ─── HMAC secret key ─────────────────────────────────────────────────────────

static SECRET_KEY: LazyLock<Vec<u8>> = LazyLock::new(load_or_generate_secret_key);

fn load_or_generate_secret_key() -> Vec<u8> {
    // 1. Environment variable (container / CI deployments).
    if let Ok(hex_key) = std::env::var("SESSION_SECRET_KEY") {
        if let Ok(bytes) = hex::decode(hex_key.trim()) {
            if bytes.len() >= 32 {
                return bytes;
            }
        }
    }
    // 2. Persisted key file.
    let path = app_config_dir().join("secret.key");
    if let Ok(hex_str) = std::fs::read_to_string(&path) {
        if let Ok(bytes) = hex::decode(hex_str.trim()) {
            if bytes.len() >= 32 {
                return bytes;
            }
        }
    }
    // 3. Generate 32 fresh bytes from two UUID v4 values.
    let u1 = uuid::Uuid::new_v4();
    let u2 = uuid::Uuid::new_v4();
    let mut key = [0u8; 32];
    key[..16].copy_from_slice(u1.as_bytes());
    key[16..].copy_from_slice(u2.as_bytes());
    let hex_str = hex::encode(key);
    if let Err(e) = std::fs::write(&path, &hex_str) {
        log::warn!("[auth] Could not persist secret key: {}", e);
    } else {
        log::info!("[auth] Generated new secret key at {}", path.display());
    }
    key.to_vec()
}

/// Sign `session_id` with HMAC-SHA256 and return `{session_id}.{hmac_hex}`.
/// The session_id is an opaque UUID — no user information is embedded.
fn sign_session_token(session_id: &str) -> String {
    let mut mac = HmacSha256::new_from_slice(&SECRET_KEY).expect("HMAC init");
    mac.update(session_id.as_bytes());
    let code = mac.finalize().into_bytes();
    format!("{}.{}", session_id, hex::encode(code))
}

/// Verify a signed token; returns the `session_id` if the signature is valid.
fn verify_session_token(token: &str) -> Option<String> {
    let (session_id, mac_hex) = token.rsplit_once('.')?;
    let mac_bytes = hex::decode(mac_hex).ok()?;
    let mut mac = HmacSha256::new_from_slice(&SECRET_KEY).ok()?;
    mac.update(session_id.as_bytes());
    mac.verify_slice(&mac_bytes).ok()?;
    Some(session_id.to_string())
}

// ─── OAuth configuration (set once at startup) ───────────────────────────────

#[derive(Clone, Debug)]
pub struct OAuthConfig {
    pub client_id: String,
    pub client_secret: String,
    pub redirect_uri: String,
}

static OAUTH_CONFIG: OnceLock<OAuthConfig> = OnceLock::new();

/// Initialise OAuth config, secret key, persistent sessions, and background
/// flush task.  Must be called once inside the async runtime before any
/// request is handled.
pub fn init_oauth(config: OAuthConfig) {
    OAUTH_CONFIG
        .set(config)
        .expect("OAuth config already initialised");
    // Eagerly initialise the secret key so any generation happens at startup.
    let _ = &*SECRET_KEY;
    // Load sessions persisted from previous runs.
    load_sessions_from_disk();
    // Spawn a background task to flush dirty sessions and prune expired ones.
    tokio::spawn(background_session_flush());
}

fn oauth_config() -> &'static OAuthConfig {
    OAUTH_CONFIG
        .get()
        .expect("OAuth config not initialised — call auth::init_oauth() at startup")
}

// ─── Session store ────────────────────────────────────────────────────────────

struct SessionEntry {
    user: UserSession,
    /// Monotonic instant used for quick in-memory expiry checks.
    expires_at: Instant,
    /// Wall-clock Unix seconds expiry (persisted to disk).
    expires_unix: i64,
    /// Wall-clock Unix seconds at session creation (for auditing).
    created_unix: i64,
    /// Set when the entry has changed and needs flushing to disk.
    dirty: bool,
    /// Nonce → received-at Unix seconds (replay-attack prevention).
    seen_nonces: HashMap<String, i64>,
}

#[derive(Serialize, Deserialize)]
struct PersistedSession {
    session_id: String,
    user: UserSession,
    expires_unix: i64,
    created_unix: i64,
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

// ─── Session persistence ──────────────────────────────────────────────────────

fn sessions_dir() -> std::path::PathBuf {
    let dir = app_config_dir().join("sessions");
    std::fs::create_dir_all(&dir).ok();
    dir
}

/// Load all non-expired sessions from disk into the in-memory store.
fn load_sessions_from_disk() {
    let dir = sessions_dir();
    let read_dir = match std::fs::read_dir(&dir) {
        Ok(d) => d,
        Err(_) => return,
    };
    let now_unix = chrono::Utc::now().timestamp();
    let mut sessions = SESSIONS.lock().expect("session lock");
    let mut loaded = 0usize;
    let mut skipped = 0usize;
    for entry in read_dir.flatten() {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("json") {
            continue;
        }
        let data = match std::fs::read_to_string(&path) {
            Ok(d) => d,
            Err(_) => continue,
        };
        let persisted: PersistedSession = match serde_json::from_str(&data) {
            Ok(p) => p,
            Err(_) => continue,
        };
        if now_unix >= persisted.expires_unix {
            // Clean up the file; it is no longer needed.
            let _ = std::fs::remove_file(&path);
            skipped += 1;
            continue;
        }
        let remaining_secs = (persisted.expires_unix - now_unix).max(0) as u64;
        let expires_at = Instant::now() + Duration::from_secs(remaining_secs);
        sessions.insert(
            persisted.session_id.clone(),
            SessionEntry {
                user: persisted.user,
                expires_at,
                expires_unix: persisted.expires_unix,
                created_unix: persisted.created_unix,
                dirty: false,
                seen_nonces: HashMap::new(),
            },
        );
        loaded += 1;
    }
    log::info!(
        "[auth] Loaded {} session(s) from disk ({} expired/removed)",
        loaded,
        skipped
    );
}

/// Flush a single session to disk via an atomic write-then-rename.
#[allow(dead_code)]
fn write_session_file(session_id: &str, entry: &SessionEntry) {
    let persisted = PersistedSession {
        session_id: session_id.to_string(),
        user: entry.user.clone(),
        expires_unix: entry.expires_unix,
        created_unix: entry.created_unix,
    };
    let dir = sessions_dir();
    let path = dir.join(format!("{}.json", session_id));
    let tmp = dir.join(format!("{}.tmp", session_id));
    if let Ok(data) = serde_json::to_string_pretty(&persisted) {
        if std::fs::write(&tmp, &data).is_ok() {
            let _ = std::fs::rename(&tmp, &path);
        }
    }
}

fn delete_session_file(session_id: &str) {
    let path = sessions_dir().join(format!("{}.json", session_id));
    let _ = std::fs::remove_file(&path);
}

/// Background task: every 5 seconds flush dirty sessions and prune expired ones.
async fn background_session_flush() {
    let mut interval = tokio::time::interval(Duration::from_secs(5));
    loop {
        interval.tick().await;
        flush_dirty_sessions();
    }
}

fn flush_dirty_sessions() {
    let now_unix = chrono::Utc::now().timestamp();
    let mut to_write: Vec<(String, PersistedSession)> = Vec::new();
    let mut to_delete: Vec<String> = Vec::new();

    {
        let mut sessions = SESSIONS.lock().expect("session lock");
        // Remove expired entries from memory and collect them for file deletion.
        sessions.retain(|sid, entry| {
            if now_unix >= entry.expires_unix {
                to_delete.push(sid.clone());
                false
            } else {
                true
            }
        });
        // Collect dirty entries to save.
        for (sid, entry) in sessions.iter_mut() {
            if entry.dirty {
                to_write.push((
                    sid.clone(),
                    PersistedSession {
                        session_id: sid.clone(),
                        user: entry.user.clone(),
                        expires_unix: entry.expires_unix,
                        created_unix: entry.created_unix,
                    },
                ));
                entry.dirty = false;
            }
        }
    }

    // Perform I/O outside the lock.
    for sid in &to_delete {
        delete_session_file(sid);
    }
    for (sid, persisted) in &to_write {
        let dir = sessions_dir();
        let path = dir.join(format!("{}.json", sid));
        let tmp = dir.join(format!("{}.tmp", sid));
        if let Ok(data) = serde_json::to_string_pretty(persisted) {
            if std::fs::write(&tmp, &data).is_ok() {
                let _ = std::fs::rename(&tmp, &path);
            }
        }
    }
    if !to_delete.is_empty() || !to_write.is_empty() {
        log::debug!(
            "[auth] Flush: saved {}, deleted {} session files",
            to_write.len(),
            to_delete.len()
        );
    }
}

// ─── Per-user preferences file I/O ───────────────────────────────────────────

fn prefs_dir(account_id: &str) -> std::path::PathBuf {
    let dir = app_config_dir().join(account_id);
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

/// Authenticate the current request and return `(session_id, UserSession)`.
///
/// Verifies the HMAC-signed session cookie, checks the 4-day sliding expiry,
/// refreshes the OAuth access token when needed, and resets the 4-day sliding
/// window (marking the session dirty for background persistence).
pub async fn current_user_session(
) -> Result<(String, UserSession), leptos::prelude::ServerFnError> {
    let headers: axum::http::HeaderMap = leptos_axum::extract()
        .await
        .map_err(|_| leptos::prelude::ServerFnError::new("Failed to extract request headers"))?;

    let raw_token = extract_raw_cookie(&headers)
        .ok_or_else(|| leptos::prelude::ServerFnError::new("Not authenticated"))?;

    let session_id = verify_session_token(&raw_token)
        .ok_or_else(|| leptos::prelude::ServerFnError::new("Invalid session token"))?;

    let now_unix = chrono::Utc::now().timestamp();

    let user = {
        let sessions = SESSIONS
            .lock()
            .map_err(|_| leptos::prelude::ServerFnError::new("Session store unavailable"))?;
        let entry = sessions
            .get(&session_id)
            .ok_or_else(|| leptos::prelude::ServerFnError::new("Session not found"))?;
        if now_unix >= entry.expires_unix {
            return Err(leptos::prelude::ServerFnError::new("Session expired"));
        }
        entry.user.clone()
    };

    // Refresh the OAuth access token if it expires within 5 minutes.
    let user = if now_unix + 300 >= user.expires_at {
        match do_refresh_token(&user.refresh_token).await {
            Ok((new_access, new_refresh, new_expiry)) => {
                let mut updated = user.clone();
                updated.access_token = new_access;
                updated.refresh_token = new_refresh;
                updated.expires_at = new_expiry;
                if let Ok(mut sessions) = SESSIONS.lock() {
                    if let Some(entry) = sessions.get_mut(&session_id) {
                        entry.user = updated.clone();
                        entry.dirty = true;
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

    // Reset the 4-day sliding window on every authenticated request.
    let new_expires_unix = now_unix + SESSION_TTL_SECS;
    let new_expires_at = Instant::now() + SESSION_TTL;
    if let Ok(mut sessions) = SESSIONS.lock() {
        if let Some(entry) = sessions.get_mut(&session_id) {
            entry.expires_unix = new_expires_unix;
            entry.expires_at = new_expires_at;
            entry.dirty = true;
        }
    }

    Ok((session_id, user))
}

/// Validate a replay-protection nonce for a state-changing request.
///
/// The `nonce` must be in the format `{unix_timestamp_secs}:{random_hex}`.
/// Checks that the timestamp is within ±5 minutes of server time and that
/// the random part has not been seen in the current replay window.
pub fn validate_nonce(
    session_id: &str,
    nonce: &str,
) -> Result<(), leptos::prelude::ServerFnError> {
    let colon = nonce
        .find(':')
        .ok_or_else(|| leptos::prelude::ServerFnError::new("Invalid nonce format"))?;
    let ts_str = &nonce[..colon];
    let id_part = &nonce[colon + 1..];

    let request_ts: i64 = ts_str
        .parse()
        .map_err(|_| leptos::prelude::ServerFnError::new("Invalid nonce timestamp"))?;

    let now_secs = chrono::Utc::now().timestamp();
    if (now_secs - request_ts).abs() > NONCE_WINDOW_SECS {
        return Err(leptos::prelude::ServerFnError::new(
            "Request timestamp out of window",
        ));
    }

    if id_part.is_empty() {
        return Err(leptos::prelude::ServerFnError::new("Empty nonce ID"));
    }

    let mut sessions = SESSIONS
        .lock()
        .map_err(|_| leptos::prelude::ServerFnError::new("Session store unavailable"))?;
    let entry = sessions
        .get_mut(session_id)
        .ok_or_else(|| leptos::prelude::ServerFnError::new("Session not found"))?;

    // Evict nonces older than the replay window.
    entry
        .seen_nonces
        .retain(|_, &mut ts| now_secs - ts < NONCE_WINDOW_SECS);

    if entry.seen_nonces.contains_key(id_part) {
        return Err(leptos::prelude::ServerFnError::new(
            "Replayed request nonce",
        ));
    }
    entry.seen_nonces.insert(id_part.to_string(), now_secs);
    Ok(())
}

/// Persist updated preferences back into the active session (in-memory + dirty flag).
pub fn update_session_prefs(session_id: &str, prefs: Settings) {
    if let Ok(mut sessions) = SESSIONS.lock() {
        if let Some(entry) = sessions.get_mut(session_id) {
            entry.user.preferences = prefs;
            entry.dirty = true;
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
        "{}?audience=api.atlassian.com&response_type=code&client_id={}&redirect_uri={}&scope={}&state={}&code_challenge={}&code_challenge_method=S256&prompt=consent",
        ATLASSIAN_AUTH_URL,
        percent_encode(&config.client_id),
        percent_encode(&config.redirect_uri),
        percent_encode(scopes),
        percent_encode(&csrf),
        percent_encode(&pkce_challenge),
    );

    log::info!(
        "[auth] Redirecting to Atlassian OAuth (client_id={}…, redirect_uri={})",
        config.client_id.chars().take(8).collect::<String>(),
        config.redirect_uri,
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

    // Validate CSRF token and retrieve PKCE verifier.
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

    let (access_token, refresh_token, expires_in) =
        match exchange_code(&code, &pkce_verifier, config).await {
            Ok(t) => t,
            Err(e) => {
                log::error!("[auth] Token exchange failed: {}", e);
                return error_redirect("token_exchange_failed");
            }
        };
    let expires_at = chrono::Utc::now().timestamp() + expires_in;

    let (cloud_id, site_url) = match fetch_accessible_resources(&access_token).await {
        Ok(r) => r,
        Err(e) => {
            log::error!("[auth] Failed to fetch accessible resources: {}", e);
            return error_redirect("resources_failed");
        }
    };

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
    let now_unix = chrono::Utc::now().timestamp();
    let expires_unix = now_unix + SESSION_TTL_SECS;

    {
        let mut sessions = SESSIONS.lock().unwrap();
        sessions.insert(
            session_id.clone(),
            SessionEntry {
                user: user.clone(),
                expires_at: Instant::now() + SESSION_TTL,
                expires_unix,
                created_unix: now_unix,
                dirty: true, // persist immediately on next flush
                seen_nonces: HashMap::new(),
            },
        );
    }

    log::info!("[auth] Session created for account_id={}", account_id);

    // Use a signed token so the cookie cannot be forged.
    let signed_token = sign_session_token(&session_id);
    let cookie_value = format!(
        "{}={}; Path=/; HttpOnly; SameSite=Strict; Max-Age={}",
        SESSION_COOKIE,
        signed_token,
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
    if let Some(raw_token) = extract_raw_cookie(&raw_headers) {
        if let Some(sid) = verify_session_token(&raw_token) {
            let account_id = {
                let mut sessions = SESSIONS.lock().unwrap();
                sessions.remove(&sid).map(|e| e.user.account_id)
            };
            if let Some(aid) = account_id {
                crate::api::cache::remove_user_cache(&aid);
                delete_session_file(&sid);
                log::info!("[auth] Logged out: {}", aid);
            }
        }
    }

    let clear_cookie = format!(
        "{}=; Path=/; HttpOnly; SameSite=Strict; Max-Age=0; Expires=Thu, 01 Jan 1970 00:00:00 GMT",
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

/// Extract the raw signed-token value from the session cookie.
fn extract_raw_cookie(headers: &HeaderMap) -> Option<String> {
    let cookie_str = headers.get("cookie")?.to_str().ok()?;
    for part in cookie_str.split(';') {
        let part = part.trim();
        if let Some(value) = part
            .strip_prefix(SESSION_COOKIE)
            .and_then(|s| s.strip_prefix('='))
        {
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
    let uuid1 = uuid::Uuid::new_v4();
    let uuid2 = uuid::Uuid::new_v4();
    let mut bytes = [0u8; 32];
    bytes[..16].copy_from_slice(uuid1.as_bytes());
    bytes[16..].copy_from_slice(uuid2.as_bytes());
    let verifier = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(bytes);
    let mut hasher = Sha256::new();
    hasher.update(verifier.as_bytes());
    let hash = hasher.finalize();
    let challenge = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(hash.as_slice());
    (verifier, challenge)
}

/// RFC 3986 percent-encoding for URL query parameter values.
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

// ─── HTTP helpers ─────────────────────────────────────────────────────────────

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
/// Atlassian uses rotating refresh tokens — the old token is invalidated
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
    let avatar_url = resp["avatarUrls"]["48x48"]
        .as_str()
        .unwrap_or("")
        .to_string();

    Ok((account_id, email, display_name, avatar_url))
}
