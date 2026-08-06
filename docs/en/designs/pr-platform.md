# PR Platform Design

> Status: draft (2026-08-04) — P6#B0 of the workspace PLAN. Companion to the
> entelecheia dogfood loop (P6#A) and the noa web UI (P6#D).

## 1. Motivation

The celestia-island family develops itself through agent pipelines (AGENTS.md
workflow: branch per task, PR, squash merge, branch delete). Today that loop
runs on third-party agent tooling, and noa — the family's own AI-native
distributed VCS — has **no pull-request abstraction at all**: no forge API
clients, no PR data model, no PR CLI. The dogfood loop (entelecheia producing
its own merged PRs) cannot close without PR creation.

The PR layer must not be GitHub-only. The design below adds a forge-agnostic
PR capability to noa, with GitHub as the first concrete backend and a
**self-hosted fallback** that lets PRs be created and merged on the user's own
noa-server when no external forge is available.

## 2. Goals and Non-Goals

### Goals

- A forge-agnostic `ForgeBackend` trait for PR lifecycle operations, mirroring
  the existing `RemoteBackend` pattern (`src/remote.rs`).
- GitHub backend (REST API) and a self-hosted backend backed by noa-server.
- PR records carry **platform-specific metadata**: model, token counts, cost —
  the markers required by the dogfood loop (P6#A2).
- `noa pr create/list/show/merge` CLI surface.
- Server-side PR store (`/api/v1/prs`) for the self-hosted path.
- Explicit failure semantics: missing tokens or unsupported forges must fail
  loudly, never silently fall back (family rule).

### Non-Goals (v1)

- In-depth review/comment threads, approvals, CI status integration.
- GitLab / Gitea backends (v2, via a generic REST adapter once the trait is
  proven by GitHub + SelfHosted).
- Merging *through* a web UI (web UI is a viewer; actions go through CLI/API).

## 3. Design Overview

```
+--------------------------------------------------------------+
| noa CLI  (noa pr create/list/show/merge [--for <forge>])     |
+--------------------------------------------------------------+
        |                             |
        v                             v
+--------------------------------+  +------------------------------+
| ForgeBackend trait             |  | ForgeConfig resolution       |
| (create_pr/list_prs/           |  | remote URL -> kind           |
|  get_pr/merge_pr)              |  | + token from env/config      |
+--------------------------------+  +------------------------------+
        |
        +--------------------+-------------------+
        v                    v                   v
+--------------+    +----------------+    +-------------------+
| GithubBackend|    | SelfHostedBknd |    | (v2) GitlabBknd   |
| REST API     |    | noa-server     |    | GiteaBknd ...     |
| (reqwest)    |    | /api/v1/prs    |    | generic adapter   |
+--------------+    +----------------+    +-------------------+
```

The trait, types, and factory live in `src/forge/` (new module). The
self-hosted PR store lives in noa-server (`src/server/`), backed by the
existing redb database. The CLI subcommand lives in `src/cli/pr_cmd.rs`.

## 4. Types

```rust
// src/forge/mod.rs

pub enum ForgeKind {
    Github,
    Gitlab,     // v2
    Gitea,      // v2
    SelfHosted, // noa-server
}

pub struct ForgeConfig {
    pub kind: ForgeKind,
    /// API base URL; derived from remote URL for known hosts, or explicit
    /// for self-hosted (e.g. http://127.0.0.1:3000).
    pub base_url: Option<String>,
    /// Inline token, or name of the env var holding it (preferred).
    pub token_env: Option<String>,
}

pub enum PrState { Open, Closed, Merged }

/// Platform-specific markers appended to PRs (dogfood requirement).
pub struct PrMetadata {
    pub model: Option<String>,
    pub input_tokens: Option<u64>,
    pub output_tokens: Option<u64>,
    pub cost_usd: Option<f64>,
}

pub struct PullRequest {
    pub id: String,        // forge-local identifier (GH number / server id)
    pub number: u64,
    pub title: String,
    pub body: String,
    pub state: PrState,
    pub base: String,      // target branch
    pub head: String,      // source branch
    pub author: String,
    pub url: String,       // web URL
    pub created_at: i64,   // unix seconds
    pub metadata: PrMetadata,
}

pub struct CreatePrRequest {
    pub base: String,
    pub head: String,
    pub title: String,
    pub body: String,
    pub metadata: Option<PrMetadata>,
}
```

## 5. ForgeBackend Trait

```rust
// src/forge/mod.rs — mirrors src/remote.rs style (async_trait, Send + Sync)

#[async_trait]
pub trait ForgeBackend: Send + Sync {
    fn kind(&self) -> ForgeKind;
    async fn create_pr(&self, cfg: &ForgeConfig, req: &CreatePrRequest) -> Result<PullRequest>;
    async fn list_prs(&self, cfg: &ForgeConfig, base: Option<&str>, state: Option<PrState>) -> Result<Vec<PullRequest>>;
    async fn get_pr(&self, cfg: &ForgeConfig, id: &str) -> Result<PullRequest>;
    async fn merge_pr(&self, cfg: &ForgeConfig, id: &str, squash: bool) -> Result<()>;
}
```

Factory, mirroring `create_remote_store` in `src/object/mod.rs`:

```rust
pub fn create_forge_backend(kind: &ForgeKind) -> Result<Box<dyn ForgeBackend>>;
```

Backend resolution rules:

1. `--for <kind>` explicit flag wins.
2. Else derive kind from the remote URL (`github.com` -> Github, known
   self-hosted URLs configured via `[remote.<name>.pr]` -> SelfHosted).
3. Unknown -> error `UnsupportedForge`, never a silent default.

> Note: `ForgeKind::SelfHosted` always requires an explicit `base_url`
> pointing at a noa-server instance; there is no URL-derived default for it.

## 6. Backends

### 6.1 GithubBackend (v1)

- HTTP: GitHub REST API via `reqwest` (already a dependency for storage).
  - `POST /repos/{owner}/{repo}/pulls`
  - `GET /repos/{owner}/{repo}/pulls?state=...`
  - `GET /repos/{owner}/{repo}/pulls/{number}`
  - `PUT /repos/{owner}/{repo}/pulls/{number}/merge` with `squash: true`
- URL parsing: `git@github.com:owner/repo.git` and
  `https://github.com/owner/repo.git` both normalized to `owner/repo`.
- Token: `GH_TOKEN`/`GITHUB_TOKEN` env, or `ForgeConfig.token_env`.
- Metadata trailers (`model:`, `usage:`, `cost:`) are appended to the PR body
  as a fenced block so they survive round-trips.
- PR title validation: reuse `noa hook validate-msg` so generated titles obey
  AGENTS.md §3 (gitmoji + capitalized + period) before the API call.

### 6.2 SelfHostedBackend (v1) — hosted on your own platform

The fallback path: PRs live on the user's own noa-server instance, no external
forge required.

- PR record model (redb table `prs`):
  `(repo_id, pr_number) -> PullRequestRecord { base_ref, head_ref, state, merge_snapshot_id }`
  — serialized with the existing rmp-serde scheme.
- Server endpoints (new, in `src/server/`):

```
POST   /api/v1/prs                create PR  (base/head refs, title, body, metadata)
GET    /api/v1/prs                list PRs    (filter base/state)
GET    /api/v1/prs/{id}           get PR
POST   /api/v1/prs/{id}/merge     merge PR    (squash: true|false)
```

- Merge semantics (server-side, no git needed): read head and base workspace
  heads, run the existing three-way merge machinery
  (`src/merge/mod.rs` `merge_trees_recursive`), write the merged tree as a new
  snapshot, update the base ref, mark the PR `Merged`. Clients later pull the
  merged state through the existing snapshot/git-export path.
- Conflicts: server returns explicit `MergeConflict` error listing conflict
  paths; PR stays `Open`. No auto-resolve (matches `ConflictResolution`
  being an explicit caller choice).
- Auth: same `NOA_API_TOKEN` bearer check as all existing `/api/v1/*` routes.

### 6.3 GitLab / Gitea (v2)

Generic REST adapter implementing the same trait, sharing
`src/forge/http.rs` helpers (token injection, error mapping, pagination).
Deferred until the trait is stabilized by v1 backends.

## 7. CLI Surface

```
noa pr create --base <b> --head <h> --title <t> [--body <f>] [--for <forge>] [--metadata json]
noa pr list [--base <b>] [--state open|closed|merged] [--for <forge>]
noa pr show <id> [--for <forge>]
noa pr merge <id> [--squash] [--for <forge>]
```

- `--metadata` accepts a JSON string (model/input_tokens/output_tokens/
  cost_usd) so agent pipelines can attach usage markers; also read from
  `NOA_PR_METADATA` env for convenience.
- Output is JSON by default (machine-consumable), human table with `--tui`
  (reuse existing ratatui patterns).

## 8. Configuration

Extend `RepoConfig` (`src/config.rs`) with an optional section:

```toml
[remote.origin.pr]
kind = "github"            # or "self-hosted"
base_url = "https://github.com/celestia-island/noa"  # optional
token_env = "GH_TOKEN"
```

Self-hosted servers are configured like remotes:

```toml
[remote.noa.pr]
kind = "self-hosted"
base_url = "http://127.0.0.1:3000"
token_env = "NOA_API_TOKEN"
```

`ForgeConfig` resolution: explicit section > URL-derived defaults
(`https://github.com/...` -> Github with default token env names).

## 9. entelecheia Integration (post-chain sequence)

The dogfood loop (P6#A3) wires PR creation into the surgery post-chain:

1. `merge_coordinator` commits with `Co-authored-by: entelecheia` trailer
   (existing) **plus** `model:`/`usage:`/`cost:` trailers (P6#A2).
2. Host-side `git push origin <branch>` — the branch is already proposed by
   the noa handshake (`entelecheia/agent-session-{uuid}`).
3. `noa pr create --base master --head <branch> --title "<gitmoji> Summary." --metadata '<json>'`
   through the configured forge (GitHub or self-hosted).
4. The returned PR URL + id is recorded into the task result / report so it
   appears in shittim-chest reports and the noa web UI.
5. `noa pr merge --squash` remains a **human-gated** action (AGENTS.md §6:
   ask before merging); the chain stops after PR creation.

`libnoa` exposes the same `ForgeBackend` surface (module `pub mod forge` in
`src/lib.rs`) so scepter can call PR creation in-process without shelling out.

## 10. plana Types Sync (P6#B4)

plana-types additions (new `ws/ui/noa_pr.rs` mirroring the existing `noa.rs`):

- `PullRequestSummary` / `PullRequestDetail` TS bindings (ts_rs export).
- Optional WS notification `Sync.PrUpdate` (created/merged) for live web UI
  refresh; consumers: noa-webui (P6#D) and shittim-chest reports channel.

## 11. Security

- Tokens never logged; only the env-var *name* is persisted in config.
- Self-hosted PR endpoints inherit the mandatory `NOA_API_TOKEN` auth and
  per-IP rate limiter from `src/server/mod.rs`.
- PR bodies are rendered as plain text in the web UI (no HTML), matching the
  family's markdown handling.
- Merge is destructive: `merge_pr` requires `--squash` to be explicit and logs
  an audit entry (server-side `POST /api/v1/prs/{id}/merge`).

## 12. Naming and Packaging (P6#D tie-in)

The noa web UI follows family conventions (checked across chest/arona/e.landing
webuis):

- Package at `packages/webui`, npm name `@celestia-island/noa-webui`
  (`"private": true`, `"type": "module"`).
- Root `pnpm-workspace.yaml` listing `packages/webui` plus
  `../hikari/packages/vue`, `../plana/packages/ui`, `../plana/packages/rpc_client`,
  with `overrides: typescript: ~6.0`.
- Deps on `@celestia-island/hikari` (H* root imports) and
  `@celestia-island/plana-ui` (P* root imports); deep subpath imports are
  `import type` only.
- Directory names: lowercase words or underscores (no camelCase/kebab dirs).
- `build` = `vue-tsc --noEmit && vite build`; outDir `../../dist/webui`.

## 13. Phasing and Acceptance

| Phase | Scope | Exit criteria |
|---|---|---|
| v1a | Types + trait + factory + GithubBackend + CLI | Unit tests: URL parsing, metadata trailers, trait dispatch; mock-server REST tests |
| v1b | SelfHostedBackend + server PR store + merge | `tests/server_api.rs` PR CRUD + conflict + merge cases |
| v2 | GitLab / Gitea generic adapter | Same trait tests green against both forges |
| dogfood | entelecheia post-chain wiring (P6#A3) | One real PR created by entelecheia via noa, with metadata, visible in web UI |

Acceptance for the full P6 loop: `celestia-integration` T5 — agent task ->
commit with trailers -> push branch -> noa PR (GitHub or self-hosted) ->
nonzero usage -> real data from `/v1/usage/period`.
