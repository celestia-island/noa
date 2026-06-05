# Object Storage Design

## Overview

noa uses a content-addressed storage model inspired by Git but with a
pluggable backend architecture. Objects are addressed by SHA-256 hash
and stored as opaque blobs.

## Object Types

### Blob

Raw file content. Identified by `SHA256(content)`.

```rust
pub struct BlobId(pub String); // hex-encoded SHA-256
```

No delta compression. Each unique content produces exactly one blob.
Duplicate content is automatically deduplicated by hash.

### Tree

Directory listing. Maps paths to child entries (blobs or subtrees).

```rust
pub struct TreeEntry {
    pub name: String,
    pub kind: TreeEntryKind, // Blob or Tree
    pub hash: String,        // SHA-256 of child
}

pub struct TreeId(pub String); // SHA-256(msgpack(entries))
```

Trees are serialized as MessagePack for compactness and fast deserialization.

## Trait Definition

```rust
#[async_trait]
pub trait ObjectStore: Send + Sync {
    async fn put_blob(&self, data: &[u8]) -> Result<BlobId>;
    async fn get_blob(&self, id: &BlobId) -> Result<Vec<u8>>;
    async fn put_tree(&self, entries: Vec<TreeEntry>) -> Result<TreeId>;
    async fn get_tree(&self, id: &TreeId) -> Result<Vec<TreeEntry>>;
}
```

## Backends

### RedbObjectStore (Local)

Uses [redb](https://github.com/cberner/redb) embedded key-value store.

- Two tables: `blobs` (key: hash bytes, value: content bytes) and
  `trees` (key: hash bytes, value: msgpack entries)
- Zero-copy reads via memory-mapped files
- ACID transactions with automatic crash recovery
- Single-writer, multi-reader via MVCC
- No external daemon required

### MinioObjectStore (Remote)

Uses S3-compatible API via `aws-sdk-s3`.

- Path-style addressing: `<bucket>/blobs/<hash>`, `<bucket>/trees/<hash>`
- Supports any S3-compatible backend (MinIO, AWS S3, GCS, etc.)
- Automatic retries with exponential backoff
- Suitable for distributed deployments

## Design Decisions

### Why SHA-256 instead of SHA-1?

Git uses SHA-1, which is cryptographically broken (SHAttered attack, 2017).
SHA-256 is collision-resistant and widely available.

### Why no delta compression?

1. **Simplicity**: Delta compression (Git's pack files) adds significant
   complexity (sliding window matching, thin packs, delta chains).
2. **Write performance**: Direct blob writes are O(1). Delta compression
   requires reading existing objects.
3. **AI agent workload**: Agents frequently regenerate entire files.
   Old versions are ephemeral — delta chains would be short and numerous.
4. **Backend offloading**: S3/MinIO handle deduplication at the storage layer.

### Why MessagePack for trees?

- 30-50% smaller than JSON for binary-heavy data
- Schema-flexible (no need for protobuf definitions)
- Rust ecosystem support via `rmp-serde`
- Fast deserialization

### Why redb over SQLite?

- **Type safety**: redb uses Rust generics for table definitions
- **Performance**: redb is optimized for Rust workloads (zero-copy reads)
- **Simplicity**: Single dependency, no C library linkage
- **Crash safety**: redb's write-ahead log is simpler than SQLite's WAL mode

Trade-off: redb has a smaller community and fewer tooling options than SQLite.
For noa's use case (embedded binary storage), the trade-off is favorable.
