//! Generic OIDC (Authorization Code + PKCE) for dashboard SSO.
//!
//! Env: `OIDC_ISSUER`, `OIDC_CLIENT_ID`, `OIDC_CLIENT_SECRET`, optional
//! `OIDC_REDIRECT_URI` / `OIDC_SCOPES` / `PUBLIC_URL`.
//!
//! Optional: `OIDC_ALLOW_PUBLIC_CLIENT=1` (empty client secret),
//! `OIDC_ALLOW_UNVERIFIED_EMAIL=1` (missing `email_verified` claim).
//!
//! ID tokens are verified against the IdP JWKS; email is taken from verified claims.

use anyhow::{bail, Context, Result};
use argon2::password_hash::rand_core::{OsRng, RngCore};
use base64::Engine;
use jsonwebtoken::{decode, decode_header, DecodingKey, Validation};
use serde::Deserialize;
use sha2::{Digest, Sha256};
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
        let issuer = std::env::var("OIDC_ISSUER")
            .ok()?
            .trim_end_matches('/')
            .to_string();
        let client_id = std::env::var("OIDC_CLIENT_ID").ok()?;
        if issuer.is_empty() || client_id.is_empty() {
            return None;
        }
        let client_secret = std::env::var("OIDC_CLIENT_SECRET").unwrap_or_default();
        let redirect_uri = std::env::var("OIDC_REDIRECT_URI").unwrap_or_else(|_| {
            let base =
                std::env::var("PUBLIC_URL").unwrap_or_else(|_| "http://localhost:3000".into());
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

    fn require_client_secret(&self) -> Result<()> {
        if self.client_secret.is_empty() && !crate::util::env_flag("OIDC_ALLOW_PUBLIC_CLIENT") {
            bail!(
                "OIDC_CLIENT_SECRET is required unless OIDC_ALLOW_PUBLIC_CLIENT=1 (or true/yes/on)"
            );
        }
        Ok(())
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
    email_verified: Option<bool>,
    #[allow(dead_code)]
    iss: Option<String>,
    #[allow(dead_code)]
    aud: serde_json::Value,
}

/// Short-lived CSRF state + PKCE verifier store: state -> (created_at, code_verifier).
static PENDING: LazyLock<Mutex<HashMap<String, (Instant, String)>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

fn prune_pending(map: &mut HashMap<String, (Instant, String)>) {
    let cutoff = Instant::now() - Duration::from_secs(600);
    map.retain(|_, (t, _)| *t > cutoff);
}

fn generate_pkce() -> (String, String) {
    let mut bytes = [0u8; 32];
    OsRng.fill_bytes(&mut bytes);
    let verifier = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(bytes);
    let digest = Sha256::digest(verifier.as_bytes());
    let challenge = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(digest);
    (verifier, challenge)
}

pub async fn authorization_url(cfg: &OidcConfig) -> Result<(String, String)> {
    cfg.require_client_secret()?;
    let disc = discover(cfg).await?;
    let state = Uuid::new_v4().to_string();
    let (verifier, challenge) = generate_pkce();
    if let Ok(mut m) = PENDING.lock() {
        prune_pending(&mut m);
        m.insert(state.clone(), (Instant::now(), verifier));
    }
    let url = format!(
        "{}?response_type=code&client_id={}&redirect_uri={}&scope={}&state={}&code_challenge={}&code_challenge_method=S256",
        disc.authorization_endpoint,
        urlencoding_encode(&cfg.client_id),
        urlencoding_encode(&cfg.redirect_uri),
        urlencoding_encode(&cfg.scopes),
        urlencoding_encode(&state),
        urlencoding_encode(&challenge),
    );
    Ok((url, state))
}

/// Take pending OIDC state and return the associated PKCE `code_verifier`.
pub fn take_pending(state: &str) -> Option<String> {
    if let Ok(mut m) = PENDING.lock() {
        prune_pending(&mut m);
        return m.remove(state).map(|(_, verifier)| verifier);
    }
    None
}

pub async fn exchange_code(cfg: &OidcConfig, code: &str, code_verifier: &str) -> Result<String> {
    cfg.require_client_secret()?;
    let disc = discover(cfg).await?;
    let jwks_uri = disc
        .jwks_uri
        .as_deref()
        .context("OIDC discovery missing jwks_uri; cannot verify ID token")?;
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(20))
        .build()?;
    let mut form = HashMap::new();
    form.insert("grant_type", "authorization_code");
    form.insert("code", code);
    form.insert("redirect_uri", cfg.redirect_uri.as_str());
    form.insert("client_id", cfg.client_id.as_str());
    form.insert("code_verifier", code_verifier);
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
    email_from_verified_jwt(cfg, &id_token, jwks_uri).await
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

    let alg = jsonwebtoken::Algorithm::RS256;
    let mut validation = Validation::new(alg);
    validation.set_issuer(&[cfg.issuer.clone()]);
    validation.set_audience(&[cfg.client_id.clone()]);
    // Some IdPs put aud as array — jsonwebtoken handles via set_audience
    validation.validate_aud = true;

    let data = decode::<IdClaims>(token, &key, &validation).context("JWT signature verify")?;
    email_from_claims(&data.claims)
}

fn email_from_claims(claims: &IdClaims) -> Result<String> {
    if claims.email_verified == Some(false) {
        bail!("OIDC email_verified claim is false");
    }
    if claims.email_verified.is_none() && !crate::util::env_flag("OIDC_ALLOW_UNVERIFIED_EMAIL") {
        bail!(
            "OIDC token missing email_verified claim; set OIDC_ALLOW_UNVERIFIED_EMAIL=1 (or true/yes/on) to allow"
        );
    }
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
