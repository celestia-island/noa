# Remote Storage Backends Design

## Overview

noa supports pluggable remote storage backends for distributing and backing up
content-addressed objects. All backends implement the same `ObjectStore` trait,
so snapshots, trees, and blobs can be pushed to any configured backend
interchangeably.

## Supported Backends

| Backend | Type Identifier | Transport | Distribution Model |
|---------|----------------|-----------|-------------------|
| Redb (local) | — (always local) | Embedded KV | None |
| IPFS (Kubo) | `ipfs` | HTTP API | Peer-to-peer (DHT, Bitswap) |
| S3 / MinIO | `s3` | S3-compatible API | Centralized object store |
| FTP / FTPS | `ftp` | FTP protocol | File server |

## Configuration

Remote backends are stored as a `[[storage]]` array in `.noa/config`:

```toml
[[storage]]
name = "ipfs-local"
type = "ipfs"
endpoint = "http://127.0.0.1:5001"
gateway = "https://ipfs.io"
auto_pin = false

[[storage]]
name = "s3-backup"
type = "s3"
endpoint = "https://s3.example.com"
bucket = "noa-objects"
access_key = "AKIA..."
secret_key = "..."
region = "us-east-1"

[[storage]]
name = "ftp-server"
type = "ftp"
endpoint = "ftp.example.com"
username = "noa"
password = "..."
port = 21
```

Each entry has a `name` (for CLI reference), a `type` discriminator, and
backend-specific fields. Unknown fields for a given type are ignored.

## Factory Pattern

`create_remote_store(&StorageConfig) -> Box<dyn ObjectStore>` inspects the
`backend_type` field and constructs the appropriate implementation:

```
type = "ipfs"  →  IpfsObjectStore  (reqwest HTTP client → Kubo daemon)
type = "s3"    →  MinioObjectStore (aws-sdk-s3 → S3-compatible endpoint)
type = "ftp"   →  FtpObjectStore   (suppaftp → FTP server)
```

## IPFS CID Bridge

noa identifies objects by hex-encoded SHA-256 hashes (`BlobId`, `TreeId`).
For IPFS, these are converted to CIDv1 (raw codec) for API calls:

```
CIDv1 bytes = [0x01]           // version 1
              [0x55]           // raw codec
              [0x12]           // sha2-256 hash function
              [0x20]           // 32-byte digest length
              [32 bytes hash]

CIDv1 string = "b" + base32_lowercase_nopad(CIDv1 bytes)
```

This conversion is a pure function — the same content always maps to the same
CID. No daemon round-trip is required for the mapping.

## Library Choice: reqwest over ipfs-api-backend-hyper

The IPFS backend uses `reqwest` directly against the Kubo HTTP API rather than
the `ipfs-api-backend-hyper` crate. Rationale:

- `aws-sdk-s3` (already a dependency) uses hyper internally; adding
  `ipfs-api-backend-hyper` risks hyper version conflicts
- The Kubo API is simple enough for thin REST calls
- `reqwest` with `rustls-tls` avoids OpenSSL system dependency

## Push Strategy

When pushing snapshots, noa walks the tree recursively:

1. For each blob in the tree: check if it exists remotely → if not, push it
2. For each subtree: recurse
3. Push the root tree
4. For IPFS with `--pin`: pin the root CID to prevent garbage collection

This ensures the complete snapshot graph is transferred. The local `RedbObjectStore`
is always the source of truth; remote backends are distribution/backup targets.

## Error Handling

Backend-specific errors are mapped to `NoaError` variants:

- `IpfsDaemonUnreachable { endpoint }` — connection refused, timeout
- `IpfsError { message }` — API error response
- `InvalidCid { cid }` — SHA-256 → CID conversion failure
- `ObjectNotFound { id }` — block not found on the network/store

## Design Decisions

### Why a flat config struct instead of tagged enums?

TOML does not have native enum support. A flat struct with a `type` discriminator
field plus optional backend-specific fields is the most TOML-friendly approach,
and matches the existing `RemoteConfig` pattern (`name` + `url` + `protocol`).

### Why not merge with `[[remotes]]`?

Git remotes (`RemoteConfig`) and object storage (`StorageConfig`) serve different
purposes:
- **Remotes** are for git protocol push/pull (source code distribution)
- **Storage** is for content-addressed object distribution (snapshots, blobs)

Keeping them separate avoids confusion and allows independent configuration.
