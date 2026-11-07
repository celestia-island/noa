//! User registry (W2c slice 1) — per-user tokens with roles, replacing the
//! single all-admin `NOA_API_TOKEN` as the primary identity surface.
//!
//! Design:
//! - `users` redb table: user name → { token_sha256, role, created_us }.
//!   The RAW token is returned exactly once at creation and never stored.
//! - `NOA_API_TOKEN` remains a bootstrap admin identity (the "root"
//!   operator), so an empty user table is still a working server.
//! - Roles: `admin` (everything incl. user management), `operator`
//!   (write routes: mirror jobs, PRs, repos), `reader` (read-only).
//! - Enforcement lives in the auth middleware: it resolves the bearer token
//!   to an `Identity` and inserts it into request extensions; handlers/
//!   sub-layers declare what they need. The /git routes resolve Basic
//!   passwords through the same table.
//! - kirino delegation is the planned successor: the role check stays a
//!   single choke point (`Identity::can`) so swapping the issuer later is
//!   a store change, not a route change.

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::sync::Arc;

use redb::{ReadableDatabase, ReadableTable, TableDefinition};

use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use serde_json::json;

use crate::error::Result;
use crate::server::handlers::AppState;

const USERS: TableDefinition<&str, &[u8]> = TableDefinition::new("users");

pub const ROLES: [&str; 3] = ["admin", "operator", "reader"];

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserEntry {
    pub name: String,
    /// sha256(hex) of the raw token — the raw token is never stored.
    pub token_sha256: String,
    /// `admin` | `operator` | `reader`.
    pub role: String,
    pub created_us: i64,
}

/// The resolved caller identity, inserted into request extensions by the
/// auth middleware.
#[derive(Debug, Clone, Serialize)]
pub struct Identity {
    pub name: String,
    pub role: String,
}

impl Identity {
    /// The bootstrap identity: `NOA_API_TOKEN` itself.
    pub fn bootstrap() -> Self {
        Identity {
            name: "bootstrap".to_string(),
            role: "admin".to_string(),
        }
    }

    pub fn is_admin(&self) -> bool {
        self.role == "admin"
    }

    /// Reader and above: every authenticated caller.
    pub fn can_read(&self) -> bool {
        true
    }

    /// Operator and above: write routes (mirror jobs, PRs, repos).
    pub fn can_write(&self) -> bool {
        self.role == "admin" || self.role == "operator"
    }

    /// Admin only: user management, destructive deletes.
    pub fn can_admin(&self) -> bool {
        self.role == "admin"
    }
}

#[derive(Clone)]
pub struct UserRegistry {
    db: Arc<redb::Database>,
}

pub fn hash_token(raw: &str) -> String {
    let digest = Sha256::digest(raw.as_bytes());
    hex::encode(digest)
}

pub fn validate_name(name: &str) -> Result<()> {
    let mut chars = name.chars();
    let first_ok = chars
        .next()
        .is_some_and(|c| c.is_ascii_lowercase() || c.is_ascii_digit());
    if !first_ok
        || name.len() > 64
        || !name
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
    {
        anyhow::bail!("user name must match ^[a-z0-9][a-z0-9-]{{0,63}}$");
    }
    Ok(())
}

pub fn validate_role(role: &str) -> Result<()> {
    if !ROLES.contains(&role) {
        anyhow::bail!("role must be one of {}", ROLES.join("|"));
    }
    Ok(())
}

impl UserRegistry {
    pub fn new(db: Arc<redb::Database>) -> Result<Self> {
        let store = UserRegistry { db };
        {
            let txn = store.db.begin_write()?;
            txn.open_table(USERS)?;
            txn.commit()?;
        }
        Ok(store)
    }

    /// Creates a user and returns the ONE-TIME raw token. Stores only the
    /// sha256. Overwrites the token (and keeps the role) if the user exists.
    pub async fn create(&self, name: &str, role: &str) -> Result<(UserEntry, String)> {
        validate_name(name)?;
        validate_role(role)?;
        // 32 random bytes → 64 hex chars. getrandom is in the dependency
        // tree via redb/rmp; use it directly to avoid a new dependency.
        let mut bytes = [0u8; 32];
        getrandom::fill(&mut bytes).map_err(|e| anyhow::anyhow!("entropy unavailable: {e}"))?;
        let raw = hex::encode(bytes);
        let token_sha256 = hash_token(&raw);
        let entry_name = name.to_string();
        let role = role.to_string();
        let db = Arc::clone(&self.db);
        let entry = tokio::task::spawn_blocking(move || -> Result<UserEntry> {
            let txn = db.begin_write()?;
            let entry = {
                let mut table = txn.open_table(USERS)?;
                let now = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_micros() as i64;
                // created_us means "when this user was first registered" —
                // a token rotation must not reset it (mirrors the repo
                // registry's created_us semantics).
                let created_us = match table.get(entry_name.as_str())?.map(|g| g.value().to_vec()) {
                    Some(bytes) => {
                        let old: UserEntry = rmp_serde::from_slice(&bytes)?;
                        old.created_us
                    }
                    None => now,
                };
                let entry = UserEntry {
                    name: entry_name.clone(),
                    token_sha256,
                    role,
                    created_us,
                };
                let data = rmp_serde::to_vec_named(&entry)?;
                table.insert(entry_name.as_str(), data.as_slice())?;
                entry
            };
            txn.commit()?;
            Ok(entry)
        })
        .await??;
        Ok((entry, raw))
    }

    pub async fn get(&self, name: &str) -> Result<Option<UserEntry>> {
        let db = Arc::clone(&self.db);
        let name = name.to_string();
        tokio::task::spawn_blocking(move || {
            let txn = db.begin_read()?;
            let table = txn.open_table(USERS)?;
            match table.get(name.as_str())? {
                Some(guard) => Ok(Some(rmp_serde::from_slice(guard.value())?)),
                None => Ok(None),
            }
        })
        .await?
    }

    pub async fn list(&self) -> Result<Vec<UserEntry>> {
        let db = Arc::clone(&self.db);
        tokio::task::spawn_blocking(move || {
            let txn = db.begin_read()?;
            let table = txn.open_table(USERS)?;
            let mut out: Vec<UserEntry> = Vec::new();
            for row in table.iter()? {
                let (_, v) = row?;
                out.push(rmp_serde::from_slice(v.value())?);
            }
            out.sort_by(|a, b| a.name.cmp(&b.name));
            Ok(out)
        })
        .await?
    }

    pub async fn delete(&self, name: &str) -> Result<bool> {
        let db = Arc::clone(&self.db);
        let name = name.to_string();
        tokio::task::spawn_blocking(move || {
            let txn = db.begin_write()?;
            let removed = txn.open_table(USERS)?.remove(name.as_str())?.is_some();
            txn.commit()?;
            Ok(removed)
        })
        .await?
    }

    /// Resolves a raw bearer token to an identity. Constant-time compare on
    /// the hash; a miss is indistinguishable from a malformed record.
    pub async fn resolve(&self, raw_token: &str) -> Option<Identity> {
        let token_sha256 = hash_token(raw_token);
        for user in self.list().await.ok()? {
            // Constant-time on the hash bytes: same primitive the middleware
            // uses for the bootstrap token.
            if constant_time_eq_hex(&user.token_sha256, &token_sha256) {
                return Some(Identity {
                    name: user.name,
                    role: user.role,
                });
            }
        }
        None
    }
}

fn constant_time_eq_hex(a: &str, b: &str) -> bool {
    let (a, b) = (a.as_bytes(), b.as_bytes());
    if a.len() != b.len() {
        return false;
    }
    let mut diff = 0u8;
    for (x, y) in a.iter().zip(b.iter()) {
        diff |= x ^ y;
    }
    diff == 0
}

// --- HTTP handlers (W2c slice 1) — all admin-only ---

fn user_json(entry: UserEntry) -> Response {
    (
        StatusCode::OK,
        Json(json!({
            "name": entry.name,
            "role": entry.role,
            "created_us": entry.created_us,
        })),
    )
        .into_response()
}

fn json_err(code: StatusCode, msg: &str) -> Response {
    (code, Json(json!({ "error": msg }))).into_response()
}

fn require_admin(req: &axum::http::Request<axum::body::Body>) -> Result<Identity, Box<Response>> {
    let identity = req.extensions().get::<Identity>().cloned().ok_or_else(|| {
        json_err(
            StatusCode::UNAUTHORIZED,
            "identity not resolved by middleware",
        )
    })?;
    if !identity.can_admin() {
        return Err(Box::new(json_err(
            StatusCode::FORBIDDEN,
            &format!("role '{}' may not manage users", identity.role),
        )));
    }
    Ok(identity)
}

pub async fn list_users(
    State(state): State<AppState>,
    req: axum::http::Request<axum::body::Body>,
) -> Response {
    if let Err(resp) = require_admin(&req) {
        return *resp;
    }
    match state.user_registry.list().await {
        Ok(users) => (
            StatusCode::OK,
            Json(json!({ "users": users
                .iter()
                .map(|u| json!({ "name": u.name, "role": u.role, "created_us": u.created_us }))
                .collect::<Vec<_>>() })),
        )
            .into_response(),
        Err(e) => json_err(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()),
    }
}

/// Creates a user. The raw token is returned EXACTLY ONCE in this response;
/// only its sha256 is stored.
pub async fn create_user(
    State(state): State<AppState>,
    req: axum::http::Request<axum::body::Body>,
) -> Response {
    if let Err(resp) = require_admin(&req) {
        return *resp;
    }
    let body = match axum::body::to_bytes(req.into_body(), 64 * 1024).await {
        Ok(b) => b,
        Err(_) => return json_err(StatusCode::BAD_REQUEST, "body read failed"),
    };
    let parsed: serde_json::Value = match serde_json::from_slice(&body) {
        Ok(v) => v,
        Err(e) => return json_err(StatusCode::BAD_REQUEST, &format!("bad JSON: {e}")),
    };
    let name = parsed.get("name").and_then(|v| v.as_str()).unwrap_or("");
    let role = parsed
        .get("role")
        .and_then(|v| v.as_str())
        .unwrap_or("reader");
    match state.user_registry.create(name, role).await {
        Ok((entry, raw_token)) => (
            StatusCode::CREATED,
            Json(json!({
                "name": entry.name,
                "role": entry.role,
                "created_us": entry.created_us,
                "token": raw_token,
                "note": "store this token now — it is not retrievable afterwards",
            })),
        )
            .into_response(),
        Err(e) => json_err(StatusCode::BAD_REQUEST, &e.to_string()),
    }
}

pub async fn get_user(
    State(state): State<AppState>,
    Path(name): Path<String>,
    req: axum::http::Request<axum::body::Body>,
) -> Response {
    if let Err(resp) = require_admin(&req) {
        return *resp;
    }
    match state.user_registry.get(&name).await {
        Ok(Some(entry)) => user_json(entry),
        Ok(None) => json_err(StatusCode::NOT_FOUND, &format!("user '{name}' not found")),
        Err(e) => json_err(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()),
    }
}

/// Deleting the bootstrap identity is impossible (it is not in the table);
/// deleting yourself IS allowed but the response says so loudly.
pub async fn delete_user(
    State(state): State<AppState>,
    Path(name): Path<String>,
    req: axum::http::Request<axum::body::Body>,
) -> Response {
    let identity = match require_admin(&req) {
        Ok(id) => id,
        Err(resp) => return *resp,
    };
    match state.user_registry.delete(&name).await {
        Ok(true) => {
            let note = if identity.name == name {
                " you deleted your own identity — your token stops working immediately"
            } else {
                ""
            };
            (StatusCode::NO_CONTENT, Json(json!({ "note": note.trim() }))).into_response()
        }
        Ok(false) => json_err(StatusCode::NOT_FOUND, &format!("user '{name}' not found")),
        Err(e) => json_err(StatusCode::INTERNAL_SERVER_ERROR, &e.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    async fn registry() -> (tempfile::TempDir, UserRegistry) {
        let dir = tempfile::tempdir().unwrap();
        let db = Arc::new(
            redb::Database::builder()
                .create(dir.path().join("t.redb"))
                .unwrap(),
        );
        (dir, UserRegistry::new(db).unwrap())
    }

    #[tokio::test]
    async fn test_create_resolve_roundtrip() {
        let (_d, reg) = registry().await;
        let (entry, raw) = reg.create("alice", "operator").await.unwrap();
        assert_eq!(entry.name, "alice");
        assert_eq!(entry.role, "operator");
        assert_ne!(raw, entry.token_sha256, "raw token must not be stored");
        assert_eq!(entry.token_sha256, hash_token(&raw));

        let id = reg.resolve(&raw).await.unwrap();
        assert_eq!(id.name, "alice");
        assert_eq!(id.role, "operator");
        assert!(id.can_write());
        assert!(!id.can_admin());
    }

    #[tokio::test]
    async fn test_resolve_rejects_wrong_token() {
        let (_d, reg) = registry().await;
        let (_e, raw) = reg.create("bob", "reader").await.unwrap();
        assert!(reg.resolve(&raw).await.is_some());
        assert!(reg.resolve("wrong-token").await.is_none());
        assert!(reg.resolve("").await.is_none());
    }

    #[tokio::test]
    async fn test_create_overwrites_token_keeps_created() {
        let (_d, reg) = registry().await;
        let (first, raw1) = reg.create("carol", "reader").await.unwrap();
        let (second, raw2) = reg.create("carol", "admin").await.unwrap();
        assert_ne!(raw1, raw2);
        assert_eq!(second.role, "admin");
        assert_eq!(second.created_us, first.created_us);
        // old token dead
        assert!(reg.resolve(&raw1).await.is_none());
        assert!(reg.resolve(&raw2).await.is_some());
    }

    #[tokio::test]
    async fn test_list_delete() {
        let (_d, reg) = registry().await;
        reg.create("a", "reader").await.unwrap();
        reg.create("b", "admin").await.unwrap();
        let all = reg.list().await.unwrap();
        assert_eq!(all.len(), 2);
        assert_eq!(all[0].name, "a");
        assert!(reg.delete("a").await.unwrap());
        assert!(reg.get("a").await.unwrap().is_none());
        assert!(reg.list().await.unwrap().len() == 1);
    }

    #[tokio::test]
    async fn test_validation() {
        let (_d, reg) = registry().await;
        assert!(reg.create("UPPER", "reader").await.is_err());
        assert!(reg.create("has space", "reader").await.is_err());
        assert!(reg.create("ok-name", "superuser").await.is_err());
    }

    #[test]
    fn test_roles_hierarchy() {
        let bootstrap = Identity::bootstrap();
        assert!(bootstrap.is_admin() && bootstrap.can_write() && bootstrap.can_admin());
        let operator = Identity {
            name: "o".into(),
            role: "operator".into(),
        };
        assert!(!operator.is_admin() && operator.can_write() && !operator.can_admin());
        let reader = Identity {
            name: "r".into(),
            role: "reader".into(),
        };
        assert!(reader.can_read() && !reader.can_write() && !reader.can_admin());
    }
}
