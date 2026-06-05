# Remote Interoperability Design

## Overview

noa supports multiple remote backends for syncing snapshots and objects
across machines and teams. The primary interoperability target is Git,
enabling seamless integration with existing workflows on GitHub, GitLab,
and Bitbucket.

## Remote Backend Trait

```rust
#[async_trait]
pub trait RemoteBackend: Send + Sync {
    async fn push_snapshots(&self, ids: &[SnapshotId]) -> Result<()>;
    async fn fetch_snapshots(&self, ids: &[SnapshotId]) -> Result<Vec<Snapshot>>;
    async fn push_objects(&self, ids: &[String]) -> Result<()>;
    async fn fetch_objects(&self, ids: &[String]) -> Result<()>;
    async fn list_refs(&self) -> Result<HashMap<String, SnapshotId>>;
    async fn update_ref(&self, name: &str, old: Option<&SnapshotId>, new: &SnapshotId) -> Result<()>;
}
```

## Git Translation Layer

The `GitTranslator` converts between noa's object model and Git's:

### Blob ↔ Git Blob

```
noa blob:  raw bytes, SHA-256 hash
Git blob:  "blob <size>\0<content>", SHA-1 hash

Translation: rehash content with Git's blob header format
```

### Tree ↔ Git Tree

```
noa tree:  MessagePack [{name, kind, hash}]
Git tree:  "<mode> <name>\0<20-byte-sha1>" entries

Translation:
  noa TreeEntry::Blob  → Git mode 100644
  noa TreeEntry::Tree  → Git mode 040000
  SHA-256 → SHA-1 rehash required
```

### Snapshot ↔ Git Commit

```
noa snapshot:
  id:        noa_abc123
  tree_hash: SHA-256
  parents:   [noa_...]
  author:    agent-001
  timestamp: 1717592400000000 (µs)
  message:   "add feature"

Git commit:
  tree:      SHA-1
  parent:    SHA-1
  author:    agent-001 <agent@noa> <unix-timestamp> <tz>
  message:   "add feature"

Translation:
  - tree_hash rehashed (SHA-256 → SHA-1)
  - parents mapped via ID lookup table
  - author formatted with dummy email
  - µs timestamp truncated to seconds
```

### Workspace ↔ Git Branch

```
noa workspace "feature-1" (head: noa_abc123)
  → Git branch "feature-1" (HEAD: git-sha1)

noa workspace "default" (head: noa_def456)
  → Git branch "main" (HEAD: git-sha1)
```

### Ref Mapping

```
noa refs:              Git refs:
  HEAD → default        HEAD → refs/heads/main
  default → noa_abc     refs/heads/main → git-sha1
  feature-1 → noa_def   refs/heads/feature-1 → git-sha2
```

## Push Flow

```
1. noa push --remote origin
2. Load all snapshots reachable from workspace heads
3. Translate each snapshot → Git commit
4. Translate each blob/tree → Git object
5. Push via git2 (libgit2) to origin URL
6. Update remote refs
```

## Pull Flow

```
1. noa pull --remote origin
2. Fetch refs via git2
3. For each new Git commit:
   a. Translate to noa snapshot
   b. Translate blobs/trees to noa objects
   c. Store in local redb
4. Create merge snapshot (local head + remote head)
5. Update workspace head
```

## MinIO/S3 Backend

For deployments without Git infrastructure:

```
noa push --remote s3-remote
  → PUT /bucket/snapshots/noa_abc123 (msgpack)
  → PUT /bucket/blobs/<sha256> (raw bytes)
  → PUT /bucket/trees/<sha256> (msgpack)
  → PUT /bucket/refs/default (snapshot ID text)
```

Advantages over Git remote:
- No tree/Snapshot translation needed (native noa format)
- Direct blob storage (no pack file overhead)
- S3-compatible (works with AWS, GCS, MinIO, Cloudflare R2)

## Remote Configuration

Stored in `.noa/config` (TOML):

```toml
[[remotes]]
name = "origin"
url = "https://github.com/example/repo.git"
backend = "git"

[[remotes]]
name = "s3"
url = "s3://my-bucket/noa-repo"
backend = "minio"
endpoint = "https://s3.amazonaws.com"
region = "us-east-1"
```

## Authentication

| Backend | Method |
|---------|--------|
| Git HTTPS | Credentials from `~/.git-credentials` or prompt |
| Git SSH | SSH agent or key file |
| MinIO/S3 | Access key + secret key (env vars or config) |

## Comparison: Remote Interop Approaches

| Approach | Used by | Pros | Cons |
|----------|---------|------|------|
| Git bridge (libgit2) | noa | Universal compatibility | Translation overhead, SHA-1/SHA-256 mismatch |
| Native protocol | Git | Fast, no translation | Only works with Git |
| WebDAV | SVN | HTTP-standard | Limited, SVN-specific |
| REST API | Bitbucket | Modern, flexible | Requires hosted service |
| S3-compatible storage | noa | Scalable, cloud-native | No Git interop without bridge |

noa supports both Git bridge (for compatibility) and native S3 (for scale).
