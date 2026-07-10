#![cfg(feature = "ssr")]

//! Bitbucket webhook receiver and registration.
//!
//! Because the app runs on localhost, Bitbucket Cloud cannot reach it
//! directly; a tunnel (ngrok / Cloudflare Tunnel) exposes a public HTTPS URL
//! set via `WEBHOOK_PUBLIC_URL`. On startup (and on later retries if first
//! attempt failed) the app reconciles a webhook registration on Bitbucket
//! pointing at
//! `{WEBHOOK_PUBLIC_URL}/webhooks/bitbucket/{token}`.
//!
//! Incoming events are HMAC-validated (`X-Hub-Signature`, SHA-256) and
//! forwarded to [`crate::api::periodic_refresh::notify_webhook_event`],
//! which debounces them and triggers a targeted refresh over the existing
//! diff + WebSocket broadcast pipeline.

use crate::api::{bitbucket, periodic_refresh};
use crate::auth;
use axum::body::Bytes;
use axum::extract::Path;
use axum::http::{HeaderMap, StatusCode};
use hmac::{Hmac, Mac};
use serde::{Deserialize, Serialize};
use sha2::Sha256;
use std::sync::LazyLock;
use std::sync::atomic::{AtomicBool, Ordering};

// ─── Configuration ──────────────────────────────────────────────────────────

/// Persisted webhook identity: the URL path token (keeps the endpoint
/// unguessable) and the HMAC secret Bitbucket signs payloads with.
#[derive(Clone, Serialize, Deserialize)]
pub struct WebhookIdentity {
    pub token: String,
    pub secret: String,
}

/// Public base URL of the tunnel, e.g. `https://abc123.ngrok-free.app`.
/// Empty/unset means webhooks are disabled and polling behaves as before.
pub fn webhook_public_url() -> Option<String> {
    std::env::var("WEBHOOK_PUBLIC_URL")
        .ok()
        .map(|s| s.trim().trim_end_matches('/').to_string())
        .filter(|s| !s.is_empty())
}

fn identity_path() -> std::path::PathBuf {
    auth::app_config_dir().join("webhook.json")
}

fn load_or_generate_identity() -> WebhookIdentity {
    let path = identity_path();
    if let Ok(raw) = std::fs::read_to_string(&path) {
        if let Ok(identity) = serde_json::from_str::<WebhookIdentity>(&raw) {
            if !identity.token.is_empty() && !identity.secret.is_empty() {
                return identity;
            }
        }
    }
    let identity = WebhookIdentity {
        token: uuid::Uuid::new_v4().simple().to_string(),
        secret: std::env::var("WEBHOOK_SECRET")
            .ok()
            .filter(|s| !s.trim().is_empty())
            .unwrap_or_else(|| uuid::Uuid::new_v4().simple().to_string()),
    };
    match serde_json::to_string_pretty(&identity) {
        Ok(json) => {
            if let Err(e) = std::fs::write(&path, json) {
                log::warn!(
                    "[webhook] could not persist identity to {}: {}",
                    path.display(),
                    e
                );
            }
        }
        Err(e) => log::warn!("[webhook] could not serialize identity: {}", e),
    }
    identity
}

static IDENTITY: LazyLock<WebhookIdentity> = LazyLock::new(load_or_generate_identity);

pub fn webhook_identity() -> &'static WebhookIdentity {
    &IDENTITY
}

/// Full endpoint URL Bitbucket should deliver events to.
fn webhook_target_url() -> Option<String> {
    webhook_public_url().map(|base| format!("{}/webhooks/bitbucket/{}", base, IDENTITY.token))
}

// ─── Registration ────────────────────────────────────────────────────────────

static REGISTRATION_DONE: AtomicBool = AtomicBool::new(false);

/// Reconcile the webhook registration on Bitbucket so exactly one hook for
/// this machine points at the current tunnel URL. Uses the server-owned
/// Bitbucket service-account credentials. Safe to call repeatedly: after the
/// first success it becomes a no-op until the process restarts.
pub async fn ensure_webhook_registration() {
    let Some(target_url) = webhook_target_url() else {
        return;
    };
    if REGISTRATION_DONE.load(Ordering::SeqCst) {
        return;
    }
    match bitbucket::reconcile_webhooks(&target_url, &IDENTITY.secret).await {
        Ok(count) => {
            REGISTRATION_DONE.store(true, Ordering::SeqCst);
            periodic_refresh::set_webhooks_active(true);
            log::info!(
                "[webhook] registration reconciled ({} hook(s)) target={}",
                count,
                target_url
            );
        }
        Err(err) => {
            log::warn!(
                "[webhook] registration failed (will retry on next session): {}",
                err
            );
        }
    }
}

/// Startup hook: register with server-owned Bitbucket credentials, if configured.
pub async fn register_on_startup() {
    if webhook_public_url().is_none() {
        log::info!("[webhook] WEBHOOK_PUBLIC_URL not set; webhooks disabled, polling only");
        return;
    }
    ensure_webhook_registration().await;
}

// ─── Receiver ────────────────────────────────────────────────────────────────

type HmacSha256 = Hmac<Sha256>;

fn signature_valid(headers: &HeaderMap, body: &[u8], secret: &str) -> bool {
    let provided = headers
        .get("x-hub-signature")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("sha256="))
        .map(str::trim);
    let Some(provided) = provided else {
        return false;
    };
    let Ok(provided_bytes) = hex::decode(provided) else {
        return false;
    };
    let Ok(mut mac) = HmacSha256::new_from_slice(secret.as_bytes()) else {
        return false;
    };
    mac.update(body);
    mac.verify_slice(&provided_bytes).is_ok()
}

fn parse_repo_full_name(body: &[u8]) -> Option<String> {
    let value: serde_json::Value = serde_json::from_slice(body).ok()?;
    value
        .get("repository")
        .and_then(|repo| repo.get("full_name"))
        .and_then(|name| name.as_str())
        .map(ToString::to_string)
}

/// Axum handler for `POST /webhooks/bitbucket/{token}`.
///
/// Responds quickly: returns 404 for a bad token, 401 for a bad signature, 200 otherwise.
pub async fn bitbucket_webhook_handler(
    Path(token): Path<String>,
    headers: HeaderMap,
    body: Bytes,
) -> StatusCode {
    if token != IDENTITY.token {
        return StatusCode::NOT_FOUND;
    }
    if !signature_valid(&headers, &body, &IDENTITY.secret) {
        log::warn!("[webhook] rejected event with missing/invalid X-Hub-Signature");
        return StatusCode::UNAUTHORIZED;
    }
    let event_key = headers
        .get("x-event-key")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("unknown")
        .to_string();
    let relevant = event_key.starts_with("repo:push") || event_key.starts_with("pullrequest:");
    if !relevant {
        log::debug!("[webhook] ignoring event {}", event_key);
        return StatusCode::OK;
    }
    let repo_full_name = parse_repo_full_name(&body);
    log::info!(
        "[webhook] received {} repo={}",
        event_key,
        repo_full_name.as_deref().unwrap_or("?")
    );
    periodic_refresh::notify_webhook_event(event_key, repo_full_name);
    StatusCode::OK
}
