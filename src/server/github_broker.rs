//! GitHub App token broker — machine identity for agent containers (2026-10-16).
//!
//! One App private key (A-class credential, 0600 file) lives HERE, on the
//! server; consumers never see it. Containers and services exchange their
//! workload bearer (the same `NOA_API_TOKEN` auth as every other route)
//! for a **short-lived GitHub installation token** (1h) that rides
//! `GH_TOKEN` straight into `gh` and git-over-https — no gh login, no
//! long-lived PAT inside any container.
//!
//! Endpoints (both behind the standard auth middleware):
//!   GET  /api/v1/github/config — `{configured}` (never echoes the key)
//!   POST /api/v1/github/token  — `{token, expires_at, installation_id}`
//!        (mint-or-cache; the cache serves until T-5min, then re-mints)
//!
//! Configuration (§3.7 A-class, env only):
//!   NOA_GITHUB_APP_ID          — the App's numeric id
//!   NOA_GITHUB_APP_KEY_PATH    — 0600 PEM file with the App private key
//!   NOA_GITHUB_INSTALLATION_ID — optional; when unset the broker
//!                                auto-discovers the org's first installation
//!
//! v1 authz = the server bearer (same as every route); workload-scoped
//! identities (per-container noa tokens) are the follow-up slice.

use axum::extract::State;
use axum::http::StatusCode;
use axum::Json;
use serde_json::{json, Value};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

fn now_unix() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs()
}

use crate::server::AppState;

const GITHUB_API: &str = "https://api.github.com";
/// GitHub-issued installation tokens live 1h; refresh 5 minutes early.
const TOKEN_TTL: Duration = Duration::from_secs(3600);
const REFRESH_MARGIN: Duration = Duration::from_secs(300);

/// Resolved broker configuration. `None` until every required piece is
/// present — every route answers the explicit "not configured" shape.
#[derive(Clone)]
pub struct GithubBroker {
    app_id: String,
    key_pem: Arc<String>,
    installation_id: Option<u64>,
    http: reqwest::Client,
    /// Per-installation token cache (multi-installation safe).
    cache: Arc<Mutex<std::collections::HashMap<u64, CachedToken>>>,
}

struct CachedToken {
    token: String,
    expires_at: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BrokerEnv {
    pub app_id: String,
    pub key_pem: String,
    pub installation_id: Option<u64>,
}

impl GithubBroker {
    /// Resolve from env; `Ok(None)` = cleanly unconfigured (the panel
    /// renders the explicit disconnected state, never a silent empty).
    pub fn from_env() -> anyhow::Result<Option<Self>> {
        let app_id = std::env::var("NOA_GITHUB_APP_ID")
            .unwrap_or_default()
            .trim()
            .to_string();
        let key_path = std::env::var("NOA_GITHUB_APP_KEY_PATH")
            .unwrap_or_default()
            .trim()
            .to_string();
        if app_id.is_empty() && key_path.is_empty() {
            return Ok(None);
        }
        let env = BrokerEnv {
            app_id,
            key_pem: std::fs::read_to_string(PathBuf::from(&key_path))
                .map_err(|e| anyhow::anyhow!("NOA_GITHUB_APP_KEY_PATH unreadable: {e}"))?,
            installation_id: std::env::var("NOA_GITHUB_INSTALLATION_ID")
                .ok()
                .and_then(|s| s.trim().parse().ok()),
        };
        Self::from_parts(env)
    }

    /// Pure constructor (unit-tested without touching env or disk).
    pub fn from_parts(env: BrokerEnv) -> anyhow::Result<Option<Self>> {
        if env.app_id.is_empty() || env.key_pem.trim().is_empty() {
            return Ok(None);
        }
        if !env.key_pem.contains("PRIVATE KEY") {
            anyhow::bail!("App key file is not a PEM private key");
        }
        Ok(Some(Self {
            app_id: env.app_id,
            key_pem: Arc::new(env.key_pem),
            installation_id: env.installation_id,
            http: reqwest::Client::builder()
                .timeout(Duration::from_secs(15))
                .build()?,
            cache: Arc::new(Mutex::new(std::collections::HashMap::new())),
        }))
    }

    pub fn configured(&self) -> bool {
        true
    }

    /// JWT claims per GitHub's App auth: issued now, valid ~10 minutes.
    fn jwt_claims(_app_id: &str, now: u64) -> (u64, u64) {
        (now.saturating_sub(60), now + 600) // (iat, exp)
    }

    fn sign_jwt(&self, now: u64) -> anyhow::Result<String> {
        use jsonwebtoken::{Algorithm, EncodingKey, Header};
        let (iat, exp) = Self::jwt_claims(&self.app_id, now);
        let claims = serde_json::json!({ "iat": iat, "exp": exp, "iss": self.app_id });
        jsonwebtoken::encode(
            &Header::new(Algorithm::RS256),
            &claims,
            &EncodingKey::from_rsa_pem(self.key_pem.as_bytes())?,
        )
        .map_err(|e| anyhow::anyhow!("jwt sign failed: {e}"))
    }

    /// Mint (or serve cached) the DEFAULT installation's token.
    pub async fn token(&self) -> anyhow::Result<(String, u64, u64)> {
        let installation = match self.installation_id {
            Some(id) => id,
            None => {
                let jwt = self.sign_jwt(now_unix())?;
                self.discover_installation(&jwt).await?
            }
        };
        self.token_for(installation).await
    }

    /// Mint (or serve cached) a SPECIFIC installation's token — the
    /// multi-installation path (each user's repos get their own token).
    pub async fn token_for(&self, installation: u64) -> anyhow::Result<(String, u64, u64)> {
        let now = now_unix();
        {
            let cache = self.cache.lock().unwrap();
            if let Some(c) = cache.get(&installation) {
                if now + REFRESH_MARGIN.as_secs() < c.expires_at {
                    return Ok((c.token.clone(), c.expires_at, installation));
                }
            }
        }
        let jwt = self.sign_jwt(now)?;
        let resp: Value = self
            .http
            .post(format!(
                "{GITHUB_API}/app/installations/{installation}/access_tokens"
            ))
            .bearer_auth(jwt)
            .header("Accept", "application/vnd.github+json")
            .header("User-Agent", "noa-server-broker")
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?;
        let token = resp
            .get("token")
            .and_then(|t| t.as_str())
            .ok_or_else(|| anyhow::anyhow!("github returned no token"))?
            .to_string();
        let expires_at = resp
            .get("expires_at")
            .and_then(|e| e.as_str())
            .and_then(parse_iso8601_to_unix)
            .unwrap_or(now + TOKEN_TTL.as_secs());
        self.cache.lock().unwrap().insert(
            installation,
            CachedToken {
                token: token.clone(),
                expires_at,
            },
        );
        Ok((token, expires_at, installation))
    }

    /// List all installations of this App (for admin/picker surfaces).
    pub async fn list_installations(&self) -> anyhow::Result<Value> {
        let jwt = self.sign_jwt(now_unix())?;
        let resp: Value = self
            .http
            .get(format!("{GITHUB_API}/app/installations"))
            .bearer_auth(jwt)
            .header("Accept", "application/vnd.github+json")
            .header("User-Agent", "noa-server-broker")
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?;
        Ok(resp)
    }

    async fn discover_installation(&self, jwt: &str) -> anyhow::Result<u64> {
        let resp: Value = self
            .http
            .get(format!("{GITHUB_API}/app/installations?per_page=1"))
            .bearer_auth(jwt)
            .header("Accept", "application/vnd.github+json")
            .header("User-Agent", "noa-server-broker")
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?;
        resp.as_array()
            .and_then(|a| a.first())
            .and_then(|i| i.get("id"))
            .and_then(|v| v.as_u64())
            .ok_or_else(|| anyhow::anyhow!("no installation found for this app"))
    }
}

/// Minimal ISO-8601 (UTC, GitHub's `2026-10-16T07:00:00Z` shape) → unix.
pub(crate) fn parse_iso8601_to_unix(s: &str) -> Option<u64> {
    let s = s.trim().trim_end_matches('Z');
    let (date, time) = s.split_once('T')?;
    let mut d = date.split('-');
    let y: i64 = d.next()?.parse().ok()?;
    let m: u64 = d.next()?.parse().ok()?;
    let dd: u64 = d.next()?.parse().ok()?;
    let mut t = time.split(':');
    let h: i64 = t.next()?.parse().ok()?;
    let mi: i64 = t.next()?.parse().ok()?;
    let se: i64 = t.next()?.parse().ok()?;
    // Days-from-civil (Howard Hinnant's canonical form): March-based
    // years, (153*mm+2)/5 month offsets — verified against Python
    // ground truth (epoch / leap day / present day) in the tests below.
    let m_i: i64 = i64::try_from(m).ok()?;
    let dd_i: i64 = i64::try_from(dd).ok()?;
    let mm: i64 = if m_i > 2 { m_i - 3 } else { m_i + 9 };
    let doy = (153 * mm + 2) / 5 + dd_i - 1;
    let y2 = if m_i > 2 { y } else { y - 1 };
    let era = y2.div_euclid(400);
    let yoe = y2 - era * 400;
    let days = era * 146_097 + yoe * 365 + yoe / 4 - yoe / 100 + doy - 719_468;
    let secs = days * 86_400 + h * 3600 + mi * 60 + se;
    u64::try_from(secs).ok()
}

// ── HTTP handlers ───────────────────────────────────────────────────

pub async fn installations(State(state): State<AppState>) -> Result<Json<Value>, StatusCode> {
    let Some(broker) = state.github_broker.as_ref() else {
        return Err(StatusCode::SERVICE_UNAVAILABLE);
    };
    match broker.list_installations().await {
        Ok(list) => Ok(Json(list)),
        Err(e) => {
            tracing::warn!(error = %e, "github installations list failed");
            Err(StatusCode::BAD_GATEWAY)
        }
    }
}

pub async fn config(State(state): State<AppState>) -> Json<Value> {
    Json(json!({ "configured": state.github_broker.is_some() }))
}

pub async fn issue_token(
    State(state): State<AppState>,
    axum::Json(body): axum::Json<Value>,
) -> Result<Json<Value>, StatusCode> {
    let Some(broker) = state.github_broker.as_ref() else {
        return Err(StatusCode::SERVICE_UNAVAILABLE);
    };
    // Multi-installation: pass {"installation_id": N} to scope the token
    // to a specific user/org's repos; omit (or null) for the default.
    let result = match body.get("installation_id").and_then(|v| v.as_u64()) {
        Some(id) => broker.token_for(id).await,
        None => broker.token().await,
    };
    match result {
        Ok((token, expires_at, installation_id)) => Ok(Json(json!({
            "token": token,
            "expires_at": expires_at,
            "installation_id": installation_id,
        }))),
        Err(e) => {
            tracing::warn!(error = %e, "github token mint failed");
            Err(StatusCode::BAD_GATEWAY)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn now() -> u64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs()
    }

    #[test]
    fn jwt_claims_window() {
        let (iat, exp) = GithubBroker::jwt_claims("123", 1_000_000);
        assert_eq!(iat, 999_940);
        assert_eq!(exp, 1_000_600);
    }

    #[test]
    fn unconfigured_when_env_absent() {
        assert!(GithubBroker::from_parts(BrokerEnv {
            app_id: String::new(),
            key_pem: String::new(),
            installation_id: None,
        })
        .unwrap()
        .is_none());
    }

    #[test]
    fn rejects_non_pem_key() {
        assert!(GithubBroker::from_parts(BrokerEnv {
            app_id: "123".into(),
            key_pem: "not a key".into(),
            installation_id: None,
        })
        .is_err());
    }

    #[test]
    fn iso8601_parse_known_values() {
        assert_eq!(
            parse_iso8601_to_unix("2026-10-16T07:00:00Z"),
            Some(1_792_134_000)
        );
        assert_eq!(parse_iso8601_to_unix("1970-01-01T00:00:00Z"), Some(0));
        assert_eq!(
            parse_iso8601_to_unix("2024-02-29T12:00:00Z"),
            Some(1_709_208_000)
        ); // leap day
        assert!(parse_iso8601_to_unix("garbage").is_none());
        assert!(parse_iso8601_to_unix("2026-10-16").is_none());
    }

    #[test]
    fn sign_jwt_shape() {
        // A throwaway 2048-bit RSA key, embedded so the test never reads
        // env or disk. Generated once for this test — not a secret.
        let pem = include_str!("../testdata/test_rsa_key.pem");
        let broker = GithubBroker::from_parts(BrokerEnv {
            app_id: "123456".into(),
            key_pem: pem.to_string(),
            installation_id: None,
        })
        .unwrap()
        .expect("valid parts");
        let jwt = broker.sign_jwt(now()).unwrap();
        let parts: Vec<&str> = jwt.split('.').collect();
        assert_eq!(parts.len(), 3, "JWS three-dot form");
        // alg header is RS256 (decode the middle-free header b64)
        use base64::Engine as _;
        let header = String::from_utf8(
            base64::engine::general_purpose::URL_SAFE_NO_PAD
                .decode(parts[0])
                .unwrap(),
        )
        .unwrap();
        assert!(header.contains("RS256"));
    }
}
