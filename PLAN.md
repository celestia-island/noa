# noa — Implementation Plan

> **Last updated**: 2026-06-05
> **Status**: Planning complete, ready for Phase 0

---

## What is noa

noa is an AI-native distributed version control system. It replaces `.git` with `.noa/`,
using a `redb` embedded KV store for metadata + content-addressed objects, and per-agent
JSONL append-only logs for zero-lock concurrent writes.

**Three design goals:**

1. **Local**: `.noa/` replaces `.git/`. Snapshot-based history. Tens to hundreds of AI agents
   can write simultaneously via isolated incremental logs — zero lock contention.
2. **Remote**: 100% compatible with Git protocol (GitHub / Bitbucket / GitLab).
3. **Self-hosted**: `noa-server` provides a native remote with MinIO-backed blob storage,
   merge queue, and agent workspace coordination.

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
│       │  │  │(git2)│ (HTTP)    │ │              │
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
| remote sync | — | git2 HTTP | GitHub / Bitbucket / GitLab |

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
// Impls: GitBackend (git2), NoaBackend (native HTTP, future)
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
3. Push via git2 (libgit2) to remote
4. Update `refs/remotes/origin/*`

### Clone flow

1. Fetch via git2 → packfile
2. Parse Git objects → insert into noa redb (blobs + trees)
3. Parse commits → create snapshots + workspaces + refs

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
edition = "2024"

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

# Async
tokio = { version = "1", features = ["full"] }
async-trait = "0.1"

# Git remote
git2 = "0.19"

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
├── snapshot/                 # ===== Snapshot engine =====
│   ├── mod.rs                # SnapshotStore trait + Snapshot struct
│   ├── redb_impl.rs          # RedbSnapshotStore
│   ├── engine.rs             # compute(agent_logs) → tree → snapshot
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
└── cli/                      # ===== CLI commands =====
    ├── mod.rs                # clap definitions + subcommand dispatch
    ├── init.rs               # noa init [--git]
    ├── status.rs             # noa status
    ├── log_cmd.rs            # noa log
    ├── snapshot_cmd.rs       # noa snapshot [create|list|diff]
    ├── workspace_cmd.rs      # noa workspace [create|switch|list|delete|merge]
    ├── remote_cmd.rs         # noa remote [add|remove|list]
    └── pushpull.rs           # noa push / noa pull / noa fetch / noa clone
```

---

## Implementation Phases

### Phase 0: Skeleton (2 days)

- [ ] Cargo workspace + dependencies
- [ ] `error.rs` — unified error types
- [ ] `config.rs` — TOML config parsing
- [ ] `repo.rs` — repository lifecycle (init, open, validate)
- [ ] `cli/init.rs` — `noa init` (create `.noa/`, init redb, create `agent-logs/`)
- [ ] Unit test scaffolding

**Deliverable**: `noa init` creates a valid `.noa/` directory structure.

### Phase 1: ObjectStore + redb (3 days)

- [ ] `object/mod.rs` — `ObjectStore` trait + `BlobId`/`TreeId`/`TreeEntries` types
- [ ] `object/redb_impl.rs` — `RedbObjectStore` (blobs + trees tables)
- [ ] Content-addressing: `BlobId = SHA256(content)`, `TreeId = SHA256(msgpack(entries))`
- [ ] Integration tests (round-trip blob, round-trip tree)

**Deliverable**: Local blob/tree CRUD via redb.

### Phase 2: AgentLog (3 days)

- [ ] `log/mod.rs` — `AgentLog` trait + `LogEntry` + `OpType`
- [ ] `log/format.rs` — `LogEntry` ↔ JSONL serialization
- [ ] `log/file_impl.rs` — `FileAgentLog` (O_APPEND + fsync)
  - Each workspace gets its own log file via UUID
  - `append()` returns monotonic seq
  - `read_since(seq)` for incremental readers
  - `read_all()` for full scan
- [ ] Concurrency stress test (100 threads simultaneous append)

**Deliverable**: Zero-lock agent log for concurrent multi-agent writes.

### Phase 3: Snapshot + Ref (3 days)

- [ ] `snapshot/mod.rs` — `SnapshotStore` trait + `Snapshot` struct
- [ ] `snapshot/redb_impl.rs` — `RedbSnapshotStore`
- [ ] `refs.rs` — `RefStore` trait + `RedbRefStore` (CAS via redb write transaction)
- [ ] `snapshot/engine.rs` — `SnapshotEngine::compute(agent_logs) → tree → snapshot`
  - Collect all entries from agent-logs
  - Apply writes/deletes/renames to build tree
  - Compute tree hash
  - Store snapshot + update workspace head
- [ ] `snapshot/diff.rs` — diff two snapshots, return file-level changes
- [ ] Unit tests

**Deliverable**: End-to-end snapshot lifecycle.

### Phase 4: Workspace Manager (3 days)

- [ ] `workspace/mod.rs` — `Workspace` struct + `WorkspaceManager`
- [ ] `workspace/ops.rs`:
  - `create(name)` — fork from HEAD snapshot
  - `switch(name)` — update HEAD, update working tree
  - `list()` — enumerate all workspaces with status
  - `delete(name)` — remove merged workspace
- [ ] HEAD / ORIG_HEAD management
- [ ] Integration tests

**Deliverable**: Multi-workspace isolation for concurrent agent work.

### Phase 5: Merge Engine (3 days)

- [ ] `merge/mod.rs` — `MergeEngine::three_way_merge(base, ours, theirs)`
  - Compare tree entries side by side
  - Determine add/modify/delete for each side
  - Same in both → no conflict
  - Different → conflict
- [ ] `merge/conflict.rs` — `ConflictDetector` + upstream-wins resolution
- [ ] `merge/consolidate.rs` — `Consolidator`
  - Read all agent-logs across workspaces
  - Sort by timestamp
  - Batch into snapshot chain
  - Write snapshots to redb
  - Mark log entries as consolidated
- [ ] Unit tests (no-conflict merge, file-level conflict, rename conflict)

**Deliverable**: Agent log consolidation + three-way merge with conflict reporting.

### Phase 6: Git Remote Compatibility (3 days)

- [ ] `remote.rs` — `RemoteBackend` trait + `PushSpec`/`FetchSpec`/`RemoteRef`
- [ ] `git/mod.rs` — `GitBackend` (impl `RemoteBackend` via git2)
- [ ] `git/import.rs` — `.git` → `.noa` import
  - Walk Git objects, translate blobs/trees/commits
  - Create corresponding snapshots + workspaces + refs
- [ ] `git/export.rs` — noa snapshot → Git packfile export
  - Recursively collect blobs and trees
  - Build Git commit objects
  - Write packfile
- [ ] `git/translate.rs` — Type translation layer
- [ ] Round-trip tests (push → clone → verify)

**Deliverable**: Push/pull/clone to GitHub/Bitbucket/GitLab.

### Phase 7: noa-server MVP (5 days)

- [ ] Server binary setup (axum + tokio)
- [ ] `object/minio_impl.rs` — `MinioObjectStore` (S3-compatible via aws-sdk-s3)
- [ ] REST API endpoints:
  - `GET/POST /refs` — list / push refs
  - `POST /blobs` — batch upload
  - `GET /blob/<hash>` — single blob
  - `POST /trees` — batch upload
  - `GET /tree/<hash>` — single tree
  - `POST /agent-log` — push incremental log
  - `GET/POST /snapshots` — list / create
  - `GET/POST /merge-queue` — merge coordination
- [ ] Server config: `object_store = "minio"` | `"redb"`
- [ ] Auth: API key / JWT
- [ ] `NoaBackend` — impl `RemoteBackend` for noa-native protocol
- [ ] Integration tests (client ↔ server round-trip)

**Deliverable**: Self-hosted noa remote with MinIO blob storage.

### Phase 8: CLI Completion (4 days)

- [ ] Full CLI command implementation:
  - `noa init [--git]` — initialize `.noa/`
  - `noa status` — current workspace state
  - `noa log [--workspace <name>]` — snapshot history
  - `noa snapshot [-m <msg>]` — create snapshot
  - `noa snapshot list` — list history
  - `noa snapshot diff <a> <b>` — file-diff between snapshots
  - `noa workspace create <name>` — new workspace
  - `noa workspace switch <name>` — change active workspace
  - `noa workspace list` — list all
  - `noa workspace delete <name>` — remove merged workspace
  - `noa workspace merge <from>` — merge from other workspace
  - `noa remote add <name> <url>` — add remote
  - `noa push [--remote <name>]` — push
  - `noa pull [--remote <name>]` — pull
  - `noa fetch [--remote <name>]` — fetch
  - `noa clone <url>` — clone remote repo
- [ ] End-to-end integration tests
- [ ] Basic documentation

**Deliverable**: Full-featured CLI for both human and agent use.

### Phase 10: MinIO ObjectStore (on-demand)

- [ ] `object/minio_impl.rs` — `MinioObjectStore`
- [ ] Config: `object_store = "minio"` with endpoint + bucket + credentials
- [ ] Integration tests with local MinIO instance

**Deliverable**: Remote blob storage for noa-server deployments.

---

## Total Estimate

| Phase | Days |
|-------|------|
| 0: Skeleton | 2 |
| 1: ObjectStore + redb | 3 |
| 2: AgentLog | 3 |
| 3: Snapshot + Ref | 3 |
| 4: Workspace Manager | 3 |
| 5: Merge Engine | 3 |
| 6: Git Remote | 3 |
| 7: noa-server MVP | 5 |
| 8: CLI Completion | 4 |
| **Total** | **29 days** |

Single developer, includes testing.

---

## Design Decisions Log

| Date | Decision | Rationale |
|------|----------|-----------|
| 2026-06-05 | noa as standalone project (not entelecheia sub-crate) | CLI tool usable independently; entelecheia calls noa via its API |
| 2026-06-05 | Working directory isolation handled by downstream (entelecheia) | noa is a pure storage engine; workspace coordination is entelecheia's job |
| 2026-06-05 | Local first + Git remote both in MVP | Ship usable Git replacement from day one |
| 2026-06-05 | `redb` as local KV store | Active, stable, typed API, competitive perf. `sled` is unmaintained beta (last release 2021) |
| 2026-06-05 | `AgentLog` as file JSONL, not redb | Zero-lock concurrent writes; single-writer redb bottleneck at scale |
| 2026-06-05 | Remote object store: MinIO (S3-compatible) | S3 is a perfect fit for content-addressed immutable objects; MinIO for self-hosted |
| 2026-06-05 | Internal format independent of Git objects | Translation layer allows format upgrades without affecting Git compatibility |
| 2026-06-05 | CLI style: Git-like subcommands | Familiar UX, lower learning curve |
| 2026-06-05 | noa-server in MVP scope | Self-hosted native remote with merge queue; not dependent on third-party platforms |
| 2026-06-05 | SQLite rejected | Not a purpose-built KV store; redb provides ACID without relational overhead |

---

## Related Projects

| Project | Path | Relationship |
|---------|------|-------------|
| **entelecheia** | `/mnt/sdb1/entelecheia` | Multi-agent orchestration. Consumes noa for version control. Container-based fork/merge model is the "heavier" alternative; noa is the lightweight local version. |
| **tairitsu** | `/mnt/sdb1/tairitsu` | WASM component model framework. Future: noa client as WASM component. |
| **kirino** | `/mnt/sdb1/kirino` | Zero-trust auth/RBAC. Used by noa-server for authentication. |
| **aoba** | `/mnt/sdb1/aoba` | Modbus debugging tool. Unrelated. |
| **hikari** | `/mnt/sdb1/hikari` | Frontend framework. Future: noa web UI. |
