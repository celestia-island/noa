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

```mermaid
graph LR
    subgraph Noa
        NB["noa blob:<br/>raw bytes<br/>SHA-256 hash"]
    end
    subgraph Git
        GB["Git blob:<br/>'blob &lt;size&gt;\\0&lt;content&gt;'<br/>SHA-1 hash"]
    end
    NB -- "rehash content with<br/>Git's blob header format" --> GB
```

### Tree ↔ Git Tree

```mermaid
graph LR
    subgraph Noa
        NT["noa tree:<br/>MessagePack [{name, kind, hash}]"]
    end
    subgraph Git
        GT["Git tree:<br/>'&lt;mode&gt; &lt;name&gt;\\0&lt;20-byte-sha1&gt;' entries"]
    end
    NT -- "TreeEntry::Blob → mode 100644<br/>TreeEntry::Tree → mode 040000<br/>SHA-256 → SHA-1 rehash" --> GT
```

### Snapshot ↔ Git Commit

```mermaid
graph LR
    subgraph "noa Snapshot"
        NS["id: noa_abc123<br/>tree_hash: SHA-256<br/>parents: [noa_...]<br/>author: agent-001<br/>timestamp: 1717592400000000 (µs)<br/>message: 'add feature'"]
    end
    subgraph "Git Commit"
        GC["tree: SHA-1<br/>parent: SHA-1<br/>author: agent-001 &lt;agent@noa&gt;<br/>message: 'add feature'"]
    end
    NS -- "tree_hash rehashed (SHA-256 → SHA-1)<br/>parents mapped via ID lookup<br/>author formatted with dummy email<br/>µs timestamp truncated to seconds" --> GC
```

### Workspace ↔ Git Branch

```mermaid
graph LR
    subgraph Noa
        NW["workspace 'feature-1'<br/>(head: noa_abc123)"]
        ND["workspace 'default'<br/>(head: noa_def456)"]
    end
    subgraph Git
        GB1["branch 'feature-1'<br/>(HEAD: git-sha1)"]
        GB2["branch 'main'<br/>(HEAD: git-sha1)"]
    end
    NW --> GB1
    ND --> GB2
```

### Ref Mapping

```mermaid
graph LR
    subgraph "noa Refs"
        NH["HEAD → default"]
        ND2["default → noa_abc"]
        NF["feature-1 → noa_def"]
    end
    subgraph "Git Refs"
        GH["HEAD → refs/heads/main"]
        GMAIN["refs/heads/main → git-sha1"]
        GF1["refs/heads/feature-1 → git-sha2"]
    end
    NH -.-> GH
    ND2 -.-> GMAIN
    NF -.-> GF1
```

## Push Flow

```mermaid
flowchart TD
    A["1. noa push --remote origin"] --> B["2. Load all snapshots reachable from workspace heads"]
    B --> C["3. Translate each snapshot → Git commit"]
    C --> D["4. Translate each blob/tree → Git object"]
    D --> E["5. Push via gix (gitoxide) to origin URL"]
    E --> F["6. Update remote refs"]
```

## Pull Flow

```mermaid
flowchart TD
    A["1. noa pull --remote origin"] --> B["2. Fetch refs via gix"]
    B --> C["3. For each new Git commit:"]
    C --> D["a. Translate to noa snapshot<br/>b. Translate blobs/trees to noa objects<br/>c. Store in local redb"]
    D --> E["4. Create merge snapshot (local head + remote head)"]
    E --> F["5. Update workspace head"]
```

## MinIO/S3 Backend

For deployments without Git infrastructure:

```mermaid
flowchart TD
    A["noa push --remote s3-remote"] --> B["PUT /bucket/snapshots/noa_abc123 (msgpack)"]
    A --> C["PUT /bucket/blobs/&lt;sha256&gt; (raw bytes)"]
    A --> D["PUT /bucket/trees/&lt;sha256&gt; (msgpack)"]
    A --> E["PUT /bucket/refs/default (snapshot ID text)"]
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
| Git bridge (gix) | noa | Universal compatibility | Translation overhead, SHA-1/SHA-256 mismatch |
| Native protocol | Git | Fast, no translation | Only works with Git |
| WebDAV | SVN | HTTP-standard | Limited, SVN-specific |
| REST API | Bitbucket | Modern, flexible | Requires hosted service |
| S3-compatible storage | noa | Scalable, cloud-native | No Git interop without bridge |

noa supports both Git bridge (for compatibility) and native S3 (for scale).
