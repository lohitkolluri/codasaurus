//! Generic OIDC (Authorization Code) for dashboard SSO.
//!
//! Env: `OIDC_ISSUER`, `OIDC_CLIENT_ID`, `OIDC_CLIENT_SECRET`, optional
//! `OIDC_REDIRECT_URI` / `OIDC_SCOPES` / `PUBLIC_URL`.
//!
//! ID tokens are verified against the IdP JWKS when available; email is taken
//! from the verified claims (falls back to payload decode only if JWKS missing).

use anyhow::{bail, Context, Result};
use jsonwebtoken::{decode, decode_header, DecodingKey, Validation};
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
    #[serde(default)]
    jwks_uri: Option<String>,
}

#[derive(Debug, Deserialize)]
struct TokenResponse {
    id_token: Option<String>,
    access_token: Option<String>,
}

#[derive(Debug, Deserialize)]
struct IdClaims {
    email: Option<String>,
    preferred_username: Option<String>,
    #[allow(dead_code)]
    iss: Option<String>,
    #[allow(dead_code)]
    aud: serde_json::Value,
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
    if let Ok(mut m) = PENDING.lock() {
        prune_pending(&mut m);
        return m.remove(state).is_some();
    }
    false
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
    let email = if let Some(jwks_uri) = disc.jwks_uri.as_deref() {
        email_from_verified_jwt(cfg, &id_token, jwks_uri).await?
    } else {
        tracing::warn!("OIDC discovery missing jwks_uri; decoding payload without signature verify");
        email_from_jwt_unverified(&id_token)?
    };
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

async fn email_from_verified_jwt(cfg: &OidcConfig, token: &str, jwks_uri: &str) -> Result<String> {
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(15))
        .build()?;
    let jwks: jsonwebtoken::jwk::JwkSet = client
        .get(jwks_uri)
        .send()
        .await?
        .error_for_status()?
        .json()
        .await
        .context("fetch JWKS")?;

    let header = decode_header(token).context("JWT header")?;
    let kid = header.kid.as_deref();
    let jwk = match kid {
        Some(kid) => jwks.find(kid).context("no JWKS key matching kid")?,
        None => jwks.keys.first().context("JWKS empty")?,
    };
    let key = DecodingKey::from_jwk(jwk).context("DecodingKey from JWK")?;

    let alg = header.alg;
    let mut validation = Validation::new(alg);
    validation.set_issuer(&[cfg.issuer.clone()]);
    validation.set_audience(&[cfg.client_id.clone()]);
    // Some IdPs put aud as array — jsonwebtoken handles via set_audience
    validation.validate_aud = true;

    let data = decode::<IdClaims>(token, &key, &validation).context("JWT signature verify")?;
    email_from_claims(&data.claims)
}

fn email_from_claims(claims: &IdClaims) -> Result<String> {
    if let Some(email) = claims.email.as_deref().filter(|e| !e.is_empty()) {
        return Ok(email.to_string());
    }
    if let Some(pref) = claims.preferred_username.as_deref() {
        if pref.contains('@') {
            return Ok(pref.to_string());
        }
    }
    bail!("OIDC token has no email claim")
}

/// Decode JWT payload without signature verification (last-resort fallback).
fn email_from_jwt_unverified(token: &str) -> Result<String> {
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
