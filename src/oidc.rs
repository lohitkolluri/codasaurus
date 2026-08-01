//! Generic OIDC (Authorization Code) for dashboard SSO.
//!
//! Env: `OIDC_ISSUER`, `OIDC_CLIENT_ID`, `OIDC_CLIENT_SECRET`, optional
//! `OIDC_REDIRECT_URI` / `OIDC_SCOPES` / `PUBLIC_URL`.
//!
//! ID tokens are decoded for the `email` claim over TLS to the IdP. For
//! stricter deployments, pin the issuer and terminate TLS at a trusted proxy;
//! JWKS signature verification can be layered on later without API changes.

use anyhow::{bail, Context, Result};
use serde::Deserialize;
use std::collections::HashMap;
use std::sync::{LazyLock, Mutex};
use std::time::{Duration, Instant};
use uuid::Uuid;

#[derive(Clone, Debug)]
pub struct OidcConfig {
    pub issuer: String,
    pub client_id: String,
    pub client_secret: String,
    pub redirect_uri: String,
    pub scopes: String,
}

impl OidcConfig {
    /// Load from env. Returns None if issuer/client_id missing (OIDC disabled).
    pub fn from_env() -> Option<Self> {
        let issuer = std::env::var("OIDC_ISSUER").ok()?.trim_end_matches('/').to_string();
        let client_id = std::env::var("OIDC_CLIENT_ID").ok()?;
        if issuer.is_empty() || client_id.is_empty() {
            return None;
        }
        let client_secret = std::env::var("OIDC_CLIENT_SECRET").unwrap_or_default();
        let redirect_uri = std::env::var("OIDC_REDIRECT_URI").unwrap_or_else(|_| {
            let base = std::env::var("PUBLIC_URL").unwrap_or_else(|_| "http://localhost:3000".into());
            format!("{}/api/auth/oidc/callback", base.trim_end_matches('/'))
        });
        let scopes = std::env::var("OIDC_SCOPES").unwrap_or_else(|_| "openid email profile".into());
        Some(Self {
            issuer,
            client_id,
            client_secret,
            redirect_uri,
            scopes,
        })
    }

    pub fn enabled() -> bool {
        Self::from_env().is_some()
    }
}

#[derive(Debug, Deserialize)]
struct Discovery {
    authorization_endpoint: String,
    token_endpoint: String,
}

#[derive(Debug, Deserialize)]
struct TokenResponse {
    id_token: Option<String>,
    access_token: Option<String>,
}

/// Short-lived CSRF state store.
static PENDING: LazyLock<Mutex<HashMap<String, Instant>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

fn prune_pending(map: &mut HashMap<String, Instant>) {
    let cutoff = Instant::now() - Duration::from_secs(600);
    map.retain(|_, t| *t > cutoff);
}

pub async fn authorization_url(cfg: &OidcConfig) -> Result<(String, String)> {
    let disc = discover(cfg).await?;
    let state = Uuid::new_v4().to_string();
    if let Ok(mut m) = PENDING.lock() {
        prune_pending(&mut m);
        m.insert(state.clone(), Instant::now());
    }
    let url = format!(
        "{}?response_type=code&client_id={}&redirect_uri={}&scope={}&state={}",
        disc.authorization_endpoint,
        urlencoding_encode(&cfg.client_id),
        urlencoding_encode(&cfg.redirect_uri),
        urlencoding_encode(&cfg.scopes),
        urlencoding_encode(&state),
    );
    Ok((url, state))
}

pub fn take_state(state: &str) -> bool {
    let Ok(mut m) = PENDING.lock() else {
        return false;
    };
    prune_pending(&mut m);
    m.remove(state).is_some()
}

pub async fn exchange_code(cfg: &OidcConfig, code: &str) -> Result<String> {
    let disc = discover(cfg).await?;
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(20))
        .build()?;
    let mut form = HashMap::new();
    form.insert("grant_type", "authorization_code");
    form.insert("code", code);
    form.insert("redirect_uri", cfg.redirect_uri.as_str());
    form.insert("client_id", cfg.client_id.as_str());
    if !cfg.client_secret.is_empty() {
        form.insert("client_secret", cfg.client_secret.as_str());
    }
    let resp = client
        .post(&disc.token_endpoint)
        .form(&form)
        .send()
        .await?
        .error_for_status()?
        .json::<TokenResponse>()
        .await?;
    let id_token = resp
        .id_token
        .or(resp.access_token)
        .context("OIDC token response missing id_token")?;
    let email = email_from_jwt(&id_token)?;
    Ok(email)
}

async fn discover(cfg: &OidcConfig) -> Result<Discovery> {
    let url = format!(
        "{}/.well-known/openid-configuration",
        cfg.issuer.trim_end_matches('/')
    );
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(15))
        .build()?;
    let disc = client
        .get(&url)
        .send()
        .await?
        .error_for_status()?
        .json::<Discovery>()
        .await?;
    Ok(disc)
}

/// Decode JWT payload (no signature verification — suitable for trusted IdP over TLS;
/// production harden with JWKS when needed).
fn email_from_jwt(token: &str) -> Result<String> {
    let parts: Vec<&str> = token.split('.').collect();
    if parts.len() < 2 {
        bail!("invalid JWT");
    }
    let payload = parts[1];
    let padded = match payload.len() % 4 {
        2 => format!("{payload}=="),
        3 => format!("{payload}="),
        _ => payload.to_string(),
    };
    let bytes = base64::Engine::decode(
        &base64::engine::general_purpose::URL_SAFE_NO_PAD,
        payload,
    )
    .or_else(|_| {
        base64::Engine::decode(&base64::engine::general_purpose::URL_SAFE, &padded)
    })
    .context("JWT payload base64")?;
    let v: serde_json::Value = serde_json::from_slice(&bytes)?;
    if let Some(email) = v.get("email").and_then(|e| e.as_str()) {
        if !email.is_empty() {
            return Ok(email.to_string());
        }
    }
    if let Some(pref) = v.get("preferred_username").and_then(|e| e.as_str()) {
        if pref.contains('@') {
            return Ok(pref.to_string());
        }
    }
    bail!("OIDC token has no email claim")
}

fn urlencoding_encode(s: &str) -> String {
    let mut out = String::with_capacity(s.len() * 2);
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char);
            }
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}
