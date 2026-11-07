//! Push policy (slice 5 of the seven-slice series): a repo-carried,
//! machine-readable `.noa/policy.toml` enforced by the SERVER at the
//! import wave — the layer that guards the actual invariants ("nothing
//! violating policy enters noa's authority or back-flows to the origin").
//!
//! Why the wave and not a pre-receive hook: the git transport accepts the
//! push into the scratch first (protocol-level acceptance), and the
//! immediate post-receive wave (slice 3) is the first point where noa
//! interprets the content. A violation there fails the wave LOUDLY and
//! reverts the offending scratch head to the origin tip — the pushed
//! content never enters noa's ref/snapshot system and never back-flows;
//! the pusher's next fetch shows the ref back at origin. This is
//! "eventual rejection" and it is documented as such.
//!
//! The policy file travels with the repository (committed at
//! `.noa/policy.toml`), so clones and mirrors carry the same rules —
//! AGENTS.md carries the human rules; policy.toml carries the enforced
//! ones.

use std::path::Path;

use serde::Deserialize;

/// The repo-carried policy. Absent file = everything disabled (a repo
/// that opts into nothing is checked for nothing) — the family repos
/// commit an explicit policy.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct PushPolicy {
    #[serde(default)]
    pub push: Option<PushRules>,
    #[serde(default)]
    pub merge: Option<MergeRules>,
}

/// Merge-time rules (slice 7): the strong form of "declare on merge" —
/// the merge is BLOCKED until a verification claim exists server-side for
/// the PR head (a run record with kind "verify" and the head sha).
#[derive(Debug, Clone, Default, Deserialize)]
pub struct MergeRules {
    #[serde(default)]
    pub require_verify_claim: bool,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct PushRules {
    /// Scan added/changed blob contents for credential shapes (PEM
    /// blocks, known token prefixes) and reject the push.
    #[serde(default)]
    pub block_credentials: bool,
    /// Require every new commit subject to match the family format:
    /// `<gitmoji> Capitalized summary.` (emoji first, space, capital).
    #[serde(default)]
    pub commit_message_format: Option<String>, // "gitmoji" | others later
}

/// Reads the policy from a git tree (`git show <rev>:.noa/policy.toml`).
/// Missing file → default (empty) policy.
pub fn load_policy(scratch: &Path, rev: &str) -> PushPolicy {
    let out = std::process::Command::new("git")
        .args([
            "-C",
            scratch.to_str().unwrap_or_default(),
            "show",
            &format!("{rev}:.noa/policy.toml"),
        ])
        .output();
    let Ok(out) = out else {
        return PushPolicy::default();
    };
    if !out.status.success() {
        return PushPolicy::default();
    }
    toml::from_str(&String::from_utf8_lossy(&out.stdout)).unwrap_or_else(|e| {
        tracing::warn!("policy.toml malformed, treating as empty: {e}");
        PushPolicy::default()
    })
}

/// One policy violation.
#[derive(Debug)]
pub struct Violation {
    pub check: &'static str,
    pub detail: String,
}

/// Checks one branch update (`old..new`) against the policy read from the
/// NEW tree. Returns the violations (empty = clean).
pub fn check_update(scratch: &Path, old: &str, new: &str, policy: &PushPolicy) -> Vec<Violation> {
    let mut violations = Vec::new();
    let Some(rules) = &policy.push else {
        return violations;
    };
    let range = format!("{old}..{new}");

    // Credential scan: changed files' contents at the NEW tree.
    if rules.block_credentials {
        if let Ok(out) = std::process::Command::new("git")
            .args([
                "-C",
                scratch.to_str().unwrap_or_default(),
                "diff",
                "--name-only",
                "--diff-filter=ACMRT",
                &range,
            ])
            .output()
        {
            if out.status.success() {
                for path in String::from_utf8_lossy(&out.stdout).lines() {
                    if path.trim().is_empty() {
                        continue;
                    }
                    if let Ok(blob) = std::process::Command::new("git")
                        .args([
                            "-C",
                            scratch.to_str().unwrap_or_default(),
                            "show",
                            &format!("{new}:{path}"),
                        ])
                        .output()
                    {
                        if blob.status.success() {
                            if let Some(hit) = credential_shape(&blob.stdout) {
                                violations.push(Violation {
                                    check: "block_credentials",
                                    detail: format!("{path}: {hit}"),
                                });
                            }
                        }
                    }
                }
            }
        }
    }

    // Commit-message format.
    if rules.commit_message_format.as_deref() == Some("gitmoji") {
        if let Ok(out) = std::process::Command::new("git")
            .args([
                "-C",
                scratch.to_str().unwrap_or_default(),
                "log",
                "--format=%s",
                &range,
            ])
            .output()
        {
            if out.status.success() {
                for subject in String::from_utf8_lossy(&out.stdout).lines() {
                    if !subject_is_gitmoji(subject) {
                        violations.push(Violation {
                            check: "commit_message_format",
                            detail: format!(
                                "{subject:?} does not match `<gitmoji> Capitalized summary.`"
                            ),
                        });
                    }
                }
            }
        }
    }

    violations
}

/// `<gitmoji> Capitalized…`: starts with a non-ASCII char (the emoji),
/// then a space, then an ASCII uppercase letter.
fn subject_is_gitmoji(s: &str) -> bool {
    let mut chars = s.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    if first.is_ascii() {
        return false;
    }
    let Some(second) = chars.next() else {
        return false;
    };
    second == ' ' && chars.next().is_some_and(|c| c.is_ascii_uppercase())
}

/// Conservative credential shapes — high precision over recall (a false
/// rejection blocks legitimate work; a miss is caught by the family's
/// human lane).
fn credential_shape(content: &[u8]) -> Option<&'static str> {
    let text = String::from_utf8_lossy(content);
    const PATTERNS: [(&str, &str); 5] = [
        ("-----BEGIN RSA PRIVATE KEY-----", "PEM private key"),
        ("-----BEGIN OPENSSH PRIVATE KEY-----", "OpenSSH private key"),
        ("-----BEGIN EC PRIVATE KEY-----", "EC private key"),
        ("-----BEGIN PRIVATE KEY-----", "PEM private key"),
        ("ghp_", "GitHub PAT (ghp_…)"),
    ];
    for (needle, label) in PATTERNS {
        if text.contains(needle) {
            return Some(label);
        }
    }
    None
}

/// Reads the policy from a NOA snapshot tree (the merge-time source: the
/// base workspace's content governs the merge). Handles both storage
/// shapes — flat names containing `/` (the importer's form) and nested
/// `Tree` entries (the snapshot engine's form).
pub async fn load_policy_from_noa_tree<O>(store: &O, tree_hash: &str) -> PushPolicy
where
    O: crate::object::ObjectStore + Clone + 'static,
{
    use crate::object::{BlobId, EntryKind, TreeId};
    let store = store.clone();
    let root = TreeId(tree_hash.to_string());
    let found = tokio::task::spawn(async move {
        let mut pending = vec![root];
        let mut policy_blob: Option<Vec<u8>> = None;
        while let Some(id) = pending.pop() {
            let tree = store.get_tree(&id).await?;
            for e in tree.0 {
                match e.kind {
                    EntryKind::Tree => {
                        pending.push(TreeId(e.id.clone()));
                    }
                    EntryKind::Gitlink => {}
                    _ => {
                        if (e.name == ".noa/policy.toml" || e.name == "policy.toml")
                            && policy_blob.is_none()
                        {
                            policy_blob = Some(store.get_blob(&BlobId(e.id.clone())).await?);
                        }
                    }
                }
            }
        }
        Ok::<_, anyhow::Error>(policy_blob)
    })
    .await
    .ok()
    .and_then(|r| r.ok())
    .flatten();
    match found {
        Some(bytes) => toml::from_str(&String::from_utf8_lossy(&bytes)).unwrap_or_else(|e| {
            tracing::warn!("policy.toml malformed, treating as empty: {e}");
            PushPolicy::default()
        }),
        None => PushPolicy::default(),
    }
}
