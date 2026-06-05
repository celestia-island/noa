# noa — Implementation Plan

> **Last updated**: 2026-06-05
> **Status**: Phases 0–7 complete; Phase 8+ (network optimization, testing) in progress

---

## What is noa

noa is an AI-native distributed version control system. It coexists with `.git` — git manages source code, noa manages AI agent iteration data. Internally it uses a `redb` embedded KV store for content-addressed objects and per-agent JSONL append-only logs for zero-lock concurrent writes.

**Three design goals:**

1. **Local**: `.noa/` alongside `.git/`. Snapshot-based history. Tens to hundreds of AI agents can write simultaneously via isolated incremental logs — zero lock contention.
2. **Remote**: 100% compatible with Git protocol (GitHub / Bitbucket / GitLab). Also supports SVN import and native `noa-server` protocol.
3. **Self-hosted**: `noa-server` provides a native remote with MinIO-backed blob storage, merge queue, and agent workspace coordination.

---

## Architecture

```
┌────────────────────────────────────────────────┐
│                noa ecosystem                    │
├────────────────────────────────────────────────┤
│                                                 │
│  ┌──────────┐    ┌──────────────────┐          │
│  │ noa CLI  │    │   noa-server     │          │
│  │ (Git-like)│    │ (axum + redb +  │          │
│  │          │    │  MinIO backend)  │          │
│  └────┬─────┘    └────────┬─────────┘          │
│       │                   │                     │
│       │  ┌────────────────┼──────┐              │
│       │  │   RemoteBackend trait │              │
│       │  │  ┌──────┬───────────┐ │              │
│       │  │  │ Git  │ Noa-native│ │              │
│       │  │  │(gix) │ (HTTP)    │ │              │
│       │  │  └──────┴───────────┘ │              │
│       │  └───────────────────────┘              │
│       │                                         │
│  ┌────┴──────────────────────────┐              │
│  │          noa-core             │              │
│  │  ┌─────────┐ ┌─────────────┐  │              │
│  │  │ redb    │ │ agent-logs/ │  │              │
│  │  │ .noa/   │ │ (JSONL)     │  │              │
│  │  │ noa.redb│ │ 零锁并行写   │  │              │
│  │  └─────────┘ └─────────────┘  │              │
│  │  ┌────────────────────────────┐│              │
│  │  │   ObjectStore trait       ││              │
│  │  │  local: redb | remote: MinIO││            │
│  │  └────────────────────────────┘│              │
│  └───────────────────────────────┘              │
│                                                 │
└────────────────────────────────────────────────┘
```

---

## Storage Design

### Responsibility Matrix

| Layer | Local (default) | Concurrency Model | Remote (optional) |
|-------|----------------|-------------------|--------------------|
| blob / tree | **redb** | MVCC multi-read | MinIO (S3-compatible) |
| snapshot metadata | **redb** | MVCC multi-read | — (local authority) |
| workspace state | **redb** | MVCC multi-read | — (local authority) |
| ref pointers | **redb** | ACID CAS | — (local authority) |
| agent incremental logs | **File JSONL** | `O_APPEND` zero-lock | — (local authority) |
| remote sync | — | gix HTTP | GitHub / Bitbucket / GitLab |

### `.noa/` Directory Layout

```
.noa/
├── noa.redb           # redb database
│                      #   blobs:       key=BlobId(bytes)    value=content(bytes)
│                      #   trees:       key=TreeId(bytes)    value=msgpack(TreeEntries)
│                      #   snapshots:   key=&str             value=msgpack(Snapshot)
│                      #   workspaces:  key=&str             value=msgpack(Workspace)
│                      #   refs:        key=&str             value=SnapshotId(bytes)
├── agent-logs/        # Per-workspace isolated log files
│   └── <ws-uuid>.log  # JSONL append-only
├── HEAD               # Text: current workspace name
├── ORIG_HEAD          # Text: previous HEAD
└── config             # TOML repository + remote configuration
```

### Why AgentLog is file-based, not redb

**Decision**: AgentLog stays as per-workspace JSONL files. Everything else goes into redb.

Rationale:

```
100 agents concurrently, each writing 10 log entries/second:

  File JSONL (per-agent, O_APPEND):
    Agent-001 → agent-logs/001.log  (exclusive fd, 0.05ms)
    Agent-002 → agent-logs/002.log  (exclusive fd, 0.05ms)
    ...
    Total: 0.05ms, zero lock contention

  redb (single writer):
    Agent-001 ─┐
    Agent-002 ─┤
    ...        ├→ single write lock queue ─⊕─ fsync(~1ms)
    Agent-100 ─┘

    1000 writes/sec × 1ms commit = 100% lock contention
    Mitigation requires batching → adds latency + crash-loss window
```

The cost of "non-uniformity" is negligible — a single trait with two impls:

```rust
trait AgentLog { append(...) -> seq; read_since(seq) -> entries; }
impl FileAgentLog  { ... }  // agent-logs/xxx.log
impl RedbAgentLog  { ... }  // reserved for future
```

### redb Table Schema

```rust
use redb::{Database, TableDefinition};

// ── Object layer ──
const BLOBS:      TableDefinition<&[u8], &[u8]>  = TableDefinition::new("blobs");
const TREES:      TableDefinition<&[u8], &[u8]>  = TableDefinition::new("trees");

// ── Snapshot layer ──
const SNAPSHOTS:  TableDefinition<&str, &[u8]>   = TableDefinition::new("snapshots");
const WORKSPACES: TableDefinition<&str, &[u8]>   = TableDefinition::new("workspaces");
const REFS:       TableDefinition<&str, &[u8]>   = TableDefinition::new("refs");
```

Data is serialized with `rmp-serde` (MessagePack) for compactness and cross-version compatibility.

---

## Core Traits

### ObjectStore

```rust
#[async_trait]
pub trait ObjectStore: Send + Sync {
    async fn put_blob(&self, content: &[u8]) -> Result<BlobId>;
    async fn get_blob(&self, id: &BlobId) -> Result<Vec<u8>>;
    async fn has_blob(&self, id: &BlobId) -> Result<bool>;
    async fn put_tree(&self, entries: &TreeEntries) -> Result<TreeId>;
    async fn get_tree(&self, id: &TreeId) -> Result<TreeEntries>;
}
// Impls: RedbObjectStore (local), MinioObjectStore (remote, S3-compatible)
```

### AgentLog

```rust
#[async_trait]
pub trait AgentLog: Send + Sync {
    async fn append(&self, entry: &LogEntry) -> Result<u64>;    // → seq
    async fn read_since(&self, seq: u64) -> Result<Vec<LogEntry>>;
    async fn read_all(&self) -> Result<Vec<LogEntry>>;
}
// Impl: FileAgentLog (JSONL, O_APPEND)
```

### SnapshotStore

```rust
#[async_trait]
pub trait SnapshotStore: Send + Sync {
    async fn get(&self, id: &SnapshotId) -> Result<Snapshot>;
    async fn store(&self, snapshot: &Snapshot) -> Result<()>;
    async fn children_of(&self, parent: &SnapshotId) -> Result<Vec<SnapshotId>>;
}
// Impl: RedbSnapshotStore
```

### RefStore

```rust
#[async_trait]
pub trait RefStore: Send + Sync {
    async fn get(&self, name: &str) -> Result<Option<SnapshotId>>;
    async fn cas(&self, name: &str, old: Option<&SnapshotId>, new: &SnapshotId) -> Result<bool>;
}
// Impl: RedbRefStore (CAS via redb write transaction)
```

### RemoteBackend

```rust
#[async_trait]
pub trait RemoteBackend: Send + Sync {
    fn protocol(&self) -> &str;
    async fn push(&self, url: &str, specs: &[PushSpec]) -> Result<PushResult>;
    async fn fetch(&self, url: &str, specs: &[FetchSpec]) -> Result<FetchResult>;
    async fn list_refs(&self, url: &str) -> Result<Vec<RemoteRef>>;
}
// Impls: GitBackend (gix), NoaBackend (native HTTP, future)
```

---

## Data Structures

### Agent Log Format (JSONL)

```jsonl
{"seq":1,"op":"write","path":"src/main.rs","blob":"abc123...","ts":1717592400000000}
{"seq":2,"op":"delete","path":"src/old.rs","ts":1717592401000000}
{"seq":3,"op":"rename","from":"src/foo.rs","to":"src/bar.rs","ts":1717592402000000}
{"seq":4,"op":"snapshot","snapshot_id":"noa_z7x9","parent":"noa_y6w8","message":"add feature","ts":1717592405000000}
{"seq":5,"op":"merge","from_ws":"agent-042","snapshot_id":"noa_a1b2","ts":1717592410000000}
```

- `seq`: agent-local monotonic counter
- `ts`: microsecond-precision timestamp
- Consolidation sorts by `ts` globally

### Snapshot

```rust
struct Snapshot {
    id: SnapshotId,           // "noa_<12-char-base62>"
    tree_hash: TreeId,        // SHA256 of tree content
    parents: Vec<SnapshotId>, // multiple = merge snapshot
    workspace: String,
    author: String,
    timestamp: Timestamp,
    message: String,
}
```

### Workspace

```rust
struct Workspace {
    name: String,
    head: SnapshotId,         // current tip snapshot
    base: SnapshotId,         // fork source snapshot
    agent_id: Option<String>,
    created_at: Timestamp,
    updated_at: Timestamp,
}
```

### LogEntry

```rust
enum OpType { Write, Delete, Rename, Snapshot, Merge }

struct LogEntry {
    seq: u64,
    op: OpType,
    path: Option<String>,
    blob_id: Option<BlobId>,
    from_path: Option<String>,     // for rename
    snapshot_id: Option<SnapshotId>,
    timestamp: Timestamp,
    message: Option<String>,
}
```

---

## Git Compatibility

noa stores data in native format (redb + JSONL), not bound to Git's object format.
Interoperability goes through a translation layer:

```
noa blob     ←─ translate ─→  Git blob   (zlib + header + content)
noa tree     ←─ translate ─→  Git tree   (binary mode+name+hash)
noa snapshot ←─ translate ─→  Git commit (tree + parent + author + msg)
```

**Advantage**: noa can upgrade internal formats (e.g. blake3 instead of sha256) without
affecting external Git compatibility.

### Push flow

1. Read target snapshot → recursively walk tree → collect all blobs
2. Convert to Git packfile format
3. Push via gix (pure Rust Git) to remote
4. Update `refs/remotes/origin/*`

### Clone flow

1. Clone via gix (or fallback to system git CLI)
2. Parse Git objects → insert into noa redb (blobs + trees)
3. Parse commits → create snapshots + workspaces + refs
4. Run `noa init` if `.noa/` doesn't exist → establish noa workspace alongside `.git/`

---

## noa-server Design

```
noa-server (axum + tokio)
├── /api/v1/repo/<name>/refs          GET   list | POST  push
├── /api/v1/repo/<name>/blobs         POST  batch upload blobs
├── /api/v1/repo/<name>/blob/<hash>   GET   get single blob
├── /api/v1/repo/<name>/trees         POST  batch upload trees
├── /api/v1/repo/<name>/tree/<hash>   GET   get single tree
├── /api/v1/repo/<name>/agent-log     POST  push incremental agent-log
├── /api/v1/repo/<name>/snapshots     GET   list | POST  create
└── /api/v1/repo/<name>/merge-queue   GET   view | POST  enqueue
```

Server's ObjectStore defaults to MinIO:

```toml
# noa-server config
[storage]
object_store = "minio"
[storage.minio]
endpoint = "https://minio.example.com"
bucket = "noa-objects"
access_key = "AKIA..."
secret_key = "sk-..."
use_tls = true
```

Server's redb stores only metadata (snapshots, refs, workspaces). All blobs and trees
go through MinIO.

---

## Crate Dependency Map

```toml
[package]
name = "noa"
version = "0.1.0"
edition = "2021"

[dependencies]
# KV storage
redb = "2"

# Serialization
serde = { version = "1", features = ["derive"] }
serde_json = "1"
rmp-serde = "1"
toml = "0.8"

# CLI
clap = { version = "4", features = ["derive"] }

# Hashing
sha2 = "0.10"
hex = "0.4"
base64 = "0.22"

# Async
tokio = { version = "1", features = ["full"] }
async-trait = "0.1"

# Git remote (pure Rust, replaces git2)
gix = "0.84"

# .gitignore parsing (ripgrep's engine — nested patterns, negation, .git/info/exclude)
ignore = "0.4"

# HTTP (noa-server)
axum = "0.8"
tower = "0.5"

# MinIO/S3 (ObjectStore remote backend)
aws-sdk-s3 = "1"
aws-config = "1"

# Utilities
anyhow = "1"
thiserror = "2"
chrono = "0.4"
uuid = { version = "1", features = ["v7"] }
tracing = "0.1"
tracing-subscriber = "0.3"
```

---

## Module Structure

```
src/
├── lib.rs                    # Top-level re-exports + prelude
├── error.rs                  # Unified error types (thiserror)
│
├── repo.rs                   # Repository lifecycle (init, open, exists, validate)
├── config.rs                 # config TOML parsing
│
├── object/                   # ===== ObjectStore trait + impls =====
│   ├── mod.rs                # trait ObjectStore + BlobId/TreeId/TreeEntries types
│   ├── redb_impl.rs          # RedbObjectStore (local)
│   └── minio_impl.rs         # MinioObjectStore (remote, S3-compatible)
│
├── log/                      # ===== AgentLog trait + impl =====
│   ├── mod.rs                # trait AgentLog + LogEntry + OpType
│   ├── file_impl.rs          # FileAgentLog (JSONL, O_APPEND)
│   └── format.rs             # LogEntry ↔ JSONL serialization
│
├── ignore.rs                 # ===== IgnoreMatcher (.gitignore + .noa exclusion) =====
│
├── snapshot/                 # ===== Snapshot engine =====
│   ├── mod.rs                # SnapshotStore trait + Snapshot struct
│   ├── redb_impl.rs          # RedbSnapshotStore
│   ├── engine.rs             # compute(agent_logs) → tree → snapshot (with ignore filter)
│   └── diff.rs               # diff(snap_a, snap_b) → Vec<FileDiff>
│
├── workspace/                # ===== Workspace manager =====
│   ├── mod.rs                # Workspace struct + WorkspaceManager
│   └── ops.rs                # create, switch, list, delete
│
├── refs.rs                   # ===== RefStore trait + redb impl =====
│
├── merge/                    # ===== Merge engine =====
│   ├── mod.rs                # three_way_merge(base, ours, theirs)
│   ├── conflict.rs           # Conflict detection + upstream-wins rules
│   └── consolidate.rs        # agent-logs → sorted → snapshot batch
│
├── git/                      # ===== Git remote compatibility =====
│   ├── mod.rs                # GitBackend (impl RemoteBackend)
│   ├── import.rs             # .git → .noa import
│   ├── export.rs             # noa snapshot → git packfile export
│   └── translate.rs          # Type translation (noa tree ↔ git tree)
│
├── remote.rs                 # ===== RemoteBackend trait =====
│
├── server/                   # ===== noa-server =====
│   ├── mod.rs                # axum router + API routes
│   └── handlers.rs           # API handlers (CRUD for refs/blobs/trees/snapshots/workspaces)
│
└── cli/                      # ===== CLI commands =====
    ├── mod.rs                # clap definitions + subcommand dispatch
    ├── init.rs               # noa init [--noa-remote <url>]
    ├── status.rs             # noa status
    ├── log_cmd.rs            # noa log
    ├── snapshot_cmd.rs       # noa snapshot [create|list|diff]
    ├── workspace_cmd.rs      # noa workspace [create|switch|list|delete|merge]
    ├── remote_cmd.rs         # noa remote [add|remove|list]
    └── pushpull.rs           # noa push / noa pull / noa fetch / noa clone
```

---

## Implementation Phases

### ✅ Phase 0: Skeleton — COMPLETE

**Delivered**:
- Cargo workspace + all dependencies
- `error.rs` — unified error types via thiserror
- `config.rs` — TOML config parsing with remote entries
- `repo.rs` — init (creates `.noa/`, redb DB, agent-logs/, HEAD, ORIG_HEAD, config), open, validate, find (walk-up), exists
- `cli/init.rs` — `noa init`
- Unit tests passing (init, open, find, store accessibility)

### ✅ Phase 1: ObjectStore + redb — COMPLETE

**Delivered**:
- `ObjectStore` trait + `BlobId`/`TreeId`/`TreeEntries` types
- `RedbObjectStore` — blobs + trees tables with SHA-256 content addressing
- `MinioObjectStore` — S3-compatible remote backend via aws-sdk-s3
- Integration tests (round-trip blob, round-trip tree)

### ✅ Phase 2: AgentLog — COMPLETE

**Delivered**:
- `AgentLog` trait + `LogEntry` + `OpType` (Write, Delete, Rename, Snapshot, Merge)
- `FileAgentLog` — per-workspace JSONL with O_APPEND zero-lock concurrent writes
- Log serialization (JSONL ↔ LogEntry), blank-line resilience
- `read_since(seq)` incremental reads, `read_all()` full scan
- Concurrency test (multi-thread simultaneous append)

### ✅ Phase 3: Snapshot + Ref — COMPLETE

**Delivered**:
- `SnapshotStore` trait + `Snapshot` struct
- `RedbSnapshotStore` — MessagePack serialization in redb
- `RedbRefStore` — CAS compare-and-swap via redb write transaction
- `SnapshotEngine::compute()` — agent log replay → tree → snapshot
- `snapshot/diff.rs` — file-diff between two snapshots (added/modified/deleted)
- Unit tests (basic compute, delete, parent chain)

### ✅ Phase 4: Workspace Manager — COMPLETE

**Delivered**:
- `Workspace` struct (name, head, base, agent_id, timestamps)
- `WorkspaceManager` — redb-backed CRUD
- Operations: create (fork from HEAD), switch, list, delete, update_head
- HEAD / ORIG_HEAD read/write management
- Default workspace ("default") creation on init
- Integration tests

### ✅ Phase 5: Merge Engine — COMPLETE

**Delivered**:
- Three-way merge (`base, ours, theirs`) comparing tree entries
- `ConflictDetector` — add/modify/delete comparisons, upstream-wins resolution
- `Consolidator` — sort all workspace logs by timestamp, batch into snapshot chain
- Unit tests (no-conflict merge, file-level conflict, rename conflict)

---

### ✅ Phase 6: Git Remote Compatibility — COMPLETE

See design docs in `docs/designs/` for detailed analysis of approach choices.

- [x] `RemoteBackend` trait + `PushSpec`/`FetchSpec`/`RemoteRef` types
- [x] `git/import.rs` — `.git` → `.noa` import (walk git tree via gix, import all blobs/trees)
- [x] `git/translate.rs` — bidirectional noa ↔ git blob/tree translation, roundtrip tested
- [x] `cli/remote_cmd.rs` — `noa remote add/remove/list`, persisted in `.noa/config`
- [x] `git/mod.rs` — `GitBackend` (impl `RemoteBackend` via system git CLI)
- [x] `git/export.rs` — noa snapshot → git working tree write + `git add/commit`
- [x] `noa clone <git-url>` — git clone (system git CLI) → auto import into noa, creates default workspace
- [x] `noa push <remote>` — export noa snapshot to git commit, push via system git CLI
- [x] `noa pull <remote>` — git pull → re-import into noa, update workspace head
- [x] `noa fetch <remote>` — `git ls-remote` to list remote refs
- [x] Git LFS support: auto-detect on clone/pull, `git lfs push --all` on push
- [x] SVN import: `noa clone --svn <url>` (svn export trunk → git init → import)
- [x] Compatibility verification: GitHub, Bitbucket (HTTPS/SSH), local bare repos

**Deliverable**: Push/pull/clone to GitHub/Bitbucket/GitLab. Hybrid model where `.git/` and `.noa/` coexist in the same working tree, with source code managed by git and agent iteration data managed by noa.

**Architectural note on clone**: `noa clone <git-url>` follows a two-step path:
1. Clone via system `git` CLI for reliability (native git protocol/submodule/LFS support)
2. Run `import_git_to_noa()` to recursively import all git tree/blobs into noa's redb
3. Create initial snapshot and default workspace pointing to the imported tree
4. Result: `.git/` and `.noa/` coexist in the cloned directory — git handles source, noa handles agent data

For a noa-native clone (`noa://` protocol), full noa-server (Phase 8) is required.

---

### Phase 7: Ignore System & Noa Remotes — COMPLETE

**Goal**: noa automatically respects existing `.gitignore` files (and other ignore sources) when computing snapshots — no `.noaignore` needed. Additionally, `noa init` auto-manages `.gitignore` (adds `.noa/`) and `.gitattributes` (adds `noa-remote` attribute) for seamless coexistence with git.

#### 7.1 Ignore System

- [x] `Cargo.toml` — add `ignore = "0.4"` (ripgrep's `.gitignore` engine)
- [x] `src/ignore.rs` — `IgnoreMatcher` module
  - `from_repo_root(root)` — collects all `.gitignore` files across directory levels
  - Also reads `.git/info/exclude` for full git compatibility
  - `should_skip(path, is_dir)` — unified check: `.noa/` internal paths always excluded + gitignore patterns
  - `is_ignored(path, is_dir)` — delegate to compiled `Gitignore` matcher
  - Caches compiled regex automata per directory (handled by `ignore` crate internals)
  - Handles nested `.gitignore`, negation patterns (`!`), directory-only patterns
  - Parent directory checking: `target/dep.rs` correctly filtered by `target/` pattern

- [x] `src/snapshot/engine.rs` — integrate ignore filter
  - Add `ignore_matcher: Option<IgnoreMatcher>` field to `SnapshotEngine`
  - Add `with_ignore(matcher) -> Self` builder method
  - In `build_tree_from_entries()`, skip entries whose path matches ignore rules

- [x] `src/cli/snapshot_cmd.rs` — pass `IgnoreMatcher` to engine when creating snapshots

#### 7.2 `.gitignore` Auto-Management

- [x] `src/repo.rs` — `manage_gitignore(root)` helper
  - On `noa init`: creates/appends `.noa/` to `.gitignore`

#### 7.3 `.gitattributes` Noa Remote Link

- [x] `src/config.rs` — add `noa_remote: Option<String>` to `RepoConfig`
- [x] `src/repo.rs` — `manage_gitattributes(root, noa_remote_url)` helper
  - On `noa init --noa-remote <url>`: writes `.gitattributes` + `.noa/config`
- [x] `src/cli/init.rs` — add `--noa-remote <url>` argument

#### 7.4 Tests

- [x] `src/ignore.rs` — 10 unit tests (all patterns, nested, exclude, wildcards, etc.)
- [x] `src/snapshot/engine.rs` — 3 integration tests (noa paths, gitignore, whitelist)
- [x] `src/repo.rs` — 6 integration tests (gitignore creation, append, dedup, gitattributes)
- [x] `tests/smoke.rs` — 13 end-to-end smoke tests
- [x] `tests/server_api.rs` — 11 API integration tests

**Deliverable**: noa transparently respects `.gitignore` for snapshot creation, auto-manages coexistence with git via `.gitignore` and `.gitattributes`, and supports `noa-remote` URL for future agent data hosting. 148 tests passing.

---

### Phase 8: noa-server MVP (in progress)

- [x] Server binary setup (axum + tokio + tower)
- [x] REST API handlers scaffolded (refs, blobs, trees, snapshots, workspaces CRUD)
- [x] `object/minio_impl.rs` — `MinioObjectStore` (S3-compatible via aws-sdk-s3)
- [ ] Wire MinIO as server's default ObjectStore (currently scaffold uses local redb)
- [ ] Auth: API key / JWT (reuse kirino zero-trust framework)
- [ ] `NoaBackend` — impl `RemoteBackend` for noa-native protocol (HTTP/JSON-RPC)
- [ ] Merge queue endpoint implementation
- [ ] Agent-log push endpoint (batch incremental log sync)
- [ ] Integration tests (client ↔ server round-trip)

**Deliverable**: Self-hosted noa remote with MinIO blob storage.

---

### Phase 9: CLI Completion (in progress)

- [x] `noa init` — initialize `.noa/`
- [x] `noa status` — current workspace state
- [x] `noa log` — snapshot history
- [x] `noa snapshot create|list|diff` — snapshot lifecycle
- [x] `noa workspace create|switch|list|delete|merge` — workspace management
- [x] `noa remote add|remove|list` — remote management
- [ ] `noa push [--remote <name>]` — push agent data (depends on Phase 6 GitBackend)
- [ ] `noa pull [--remote <name>]` — pull agent data (depends on Phase 6)
- [ ] `noa fetch [--remote <name>]` — fetch remote refs (depends on Phase 6)
- [ ] `noa clone <url>` — clone remote to local (depends on Phase 6)
- [x] End-to-end workflow examples (basic, multi-agent, merge, remote-sync)
- [ ] Basic documentation

**Deliverable**: Full-featured CLI for both human and agent use.

---

### Phase 10: entelecheia Integration (future)

noa is consumed by entelecheia (the multi-agent orchestration platform). Integration points:

- [ ] **GitRemote → `noa init`**: When entelecheia clones a git workspace, automatically run `noa init` in the cloned directory to establish a parallel `.noa/` workspace alongside `.git/`
- [ ] **Agent file writes → ignore check**: Before agents write files to noa, consult the `IgnoreMatcher` (or call `noa check-ignore <path>`) to avoid ingesting build artifacts / secrets
- [ ] **Read `noa-remote` from `.gitattributes`**: When syncing agent iteration data, read the noa remote URL from `.gitattributes` to determine the sync target
- [ ] **Container agent log isolation**: Each container-backed agent writes to its own workspace log in `.noa/agent-logs/<workspace>.log`
- [ ] **Noa-native workspace type**: Add `NoaRemote` as a new `WorkspaceConnectionKind` (alongside existing LocalFilesystem, DockerVolume, PolemosRemote, GitRemote)

---

## Total Estimate

| Phase | Days | Status |
|-------|------|--------|
| 0: Skeleton | 2 | ✅ Complete |
| 1: ObjectStore + redb | 3 | ✅ Complete |
| 2: AgentLog | 3 | ✅ Complete |
| 3: Snapshot + Ref | 3 | ✅ Complete |
| 4: Workspace Manager | 3 | ✅ Complete |
| 5: Merge Engine | 3 | ✅ Complete |
| 6: Git Remote | 5 | ✅ Complete |
| 7: Ignore System + Noa Remotes | 3 | ✅ Complete |
| 8: noa-server MVP (remaining) | 4 | In progress |
| 9: CLI Completion (remaining) | 2 | In progress |
| 10: entelecheia Integration | 3 | Future |
| **Total remaining** | **~9 days** | |

Single developer, includes testing.

### Test Coverage

| Suite | Count | Status |
|-------|-------|--------|
| Unit tests (lib) | 124 | ✅ Passing |
| Smoke tests (E2E) | 13 | ✅ Passing |
| Server API integration | 11 | ✅ Passing |
| Compatibility (LFS/Bitbucket/SVN) | 8 | ✅ 6 passed, 2 ignored |
| **Total** | **156** | **154 passed, 0 failed, 2 ignored** |

Single developer, includes testing.

---

## Design Decisions Log

| Date | Decision | Rationale |
|------|----------|-----------|
| 2026-06-05 | noa as standalone project (not entelecheia sub-crate) | CLI tool usable independently; entelecheia calls noa via its API |
| 2026-06-05 | Local first + Git remote both in MVP | Ship usable Git replacement from day one |
| 2026-06-05 | `redb` as local KV store | Active, stable, typed API, competitive perf. `sled` is unmaintained beta (last release 2021) |
| 2026-06-05 | `AgentLog` as file JSONL, not redb | Zero-lock concurrent writes; single-writer redb bottleneck at scale |
| 2026-06-05 | Remote object store: MinIO (S3-compatible) | S3 is a perfect fit for content-addressed immutable objects; MinIO for self-hosted |
| 2026-06-05 | Internal format independent of Git objects | Translation layer allows format upgrades without affecting Git compatibility |
| 2026-06-05 | CLI style: Git-like subcommands | Familiar UX, lower learning curve |
| 2026-06-05 | noa-server in MVP scope | Self-hosted native remote with merge queue; not dependent on third-party platforms |
| 2026-06-05 | SQLite rejected | Not a purpose-built KV store; redb provides ACID without relational overhead |
| 2026-06-05 | Migrate git2 → gix (pure Rust) | No libgit2 C dependency; already at v0.84; full git protocol support |
| 2026-06-05 | Ignore filtering at snapshot time (not ingestion) | Single point of change in engine.rs; rebuild snapshot auto-applies latest ignore; no agent code changes |
| 2026-06-05 | No `.noaignore` — piggyback on `.gitignore` | Projects already have `.gitignore`; no need to maintain duplicate ignore patterns. `ignore` crate handles full gitignore spec |
| 2026-06-05 | `noa-remote` in `.gitattributes` (dual storage with `.noa/config`) | Visible in source tree, versioned, git-compatible; same pattern as Git LFS's `filter=lfs` |
| 2026-06-05 | Both gix + system git CLI for clone | gix preferred (pure Rust), but fallback to system git CLI ensures reliability for edge cases |
| 2026-06-05 | System git CLI for network ops (push/pull/fetch) | git protocol is complex; system CLI is battle-tested. gix used only for local tree traversal |
| 2026-06-05 | `SnapshotEngine.with_repo_root()` to store actual blob content | Agent logs only record SHA-256 hashes; engine reads real files from working tree and stores blobs via `put_blob` |
| 2026-06-05 | SVN: one-way import via `svn export` + `git init` | git-svn not available in conda; `svn export trunk` is simpler and sufficient for migration scenarios |
| 2026-06-05 | Git LFS: use system `git lfs` CLI hooks | `git lfs install/pull/push` called after clone/pull/push; no need to reimplement LFS protocol |
| 2026-06-05 | `noa pull` updates workspace head after import | Previously left head at `noa_empty`; now correctly points to imported snapshot |
| 2026-06-05 | Package renamed to `libnoa` (crates.io: `noa` already taken) | Binary name unchanged (`noa`); `[[bin]] name` determines output filename, not package name |
| 2026-06-05 | Distribution via install scripts (bash + PowerShell) | `scripts/install.sh` and `scripts/install.ps1` for one-liner install from GitHub Releases or from-source fallback |

---

## Related Projects

| Project | Path | Relationship |
|---------|------|-------------|
| **entelecheia** | `/mnt/sdb1/entelecheia` | Multi-agent orchestration. Consumes noa for version control. Container-based fork/merge model is the "heavier" alternative; noa is the lightweight local version. Phase 10 integration planned. |
| **tairitsu** | `/mnt/sdb1/tairitsu` | WASM component model framework. Future: noa client as WASM component. |
| **kirino** | `/mnt/sdb1/kirino` | Zero-trust auth/RBAC. Used by noa-server for authentication. |
| **aoba** | `/mnt/sdb1/aoba` | Modbus debugging tool. Unrelated. |
| **hikari** | `/mnt/sdb1/hikari` | Frontend framework. Future: noa web UI. |
