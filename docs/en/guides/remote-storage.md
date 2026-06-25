# Remote Storage Guide

## Overview

noa supports multiple remote storage backends for distributing and backing up
content-addressed objects. Backends are configured per-repository and managed
through the unified `noa storage` command.

## Supported Backends

| Backend | Type | Requires | Use Case |
|---------|------|----------|----------|
| IPFS (Kubo) | `ipfs` | Running IPFS daemon | Decentralized P2P distribution |
| S3 / MinIO | `s3` | S3-compatible endpoint | Centralized backup, cloud storage |

## Adding a Storage Backend

### IPFS

First, start a Kubo daemon:

```bash
ipfs daemon &   # listens on 127.0.0.1:5001
```

Add the backend:

```bash
# Add IPFS with defaults (endpoint=http://127.0.0.1:5001, gateway=https://ipfs.io)
noa storage add ipfs-local --type ipfs

# Customize endpoint and gateway
noa storage add ipfs-local --type ipfs \
  --endpoint http://192.168.1.100:5001 \
  --gateway https://dweb.link

# Use a remote pinning service (e.g., Pinata)
noa storage add pinata --type ipfs \
  --endpoint https://api.pinata.cloud/psa \
  --auth-token YOUR_PINATA_TOKEN

# Enable automatic pinning on every push
noa storage add ipfs-local --type ipfs --auto-pin
```

### S3 / MinIO

```bash
# Add an S3-compatible backend
noa storage add s3-backup --type s3 \
  --endpoint https://s3.us-east-1.amazonaws.com \
  --bucket my-noa-objects \
  --access-key AKIAIOSFODNN7EXAMPLE \
  --secret-key wJalrXUtnFEMI/K7MDENG/bPxRfiCYEXAMPLEKEY \
  --region us-east-1

# Add a local MinIO server
noa storage add minio-local --type s3 \
  --endpoint http://localhost:9000 \
  --bucket noa \
  --access-key minioadmin \
  --secret-key minioadmin
```

## Managing Backends

```bash
# List all configured backends
noa storage list
# ipfs-local    http://127.0.0.1:5001 (ipfs) [gateway=https://ipfs.io]
# s3-backup     https://s3.example.com (s3) [bucket=noa-objects]

# Check connection status
noa storage status               # all backends
noa storage status ipfs-local    # specific backend

# Remove a backend
noa storage remove s3-backup
```

## Pushing Snapshots

Push objects to a remote backend for distribution or backup:

```bash
# Push all snapshots to a specific backend
noa storage push --target ipfs-local

# Push and pin (IPFS only — prevents garbage collection)
noa storage push --target ipfs-local --pin

# Push a specific snapshot
noa storage push --target s3-backup --snapshot noa_abc123

# Push all snapshots from a workspace
noa storage push --target ipfs-local --workspace feature-auth --pin
```

With `auto_pin = true` in config, `--pin` is implied. You can also push to
all auto-pin backends at once by omitting `--target`:

```bash
noa storage push --pin   # pushes to all backends with auto_pin=true
```

## Fetching Objects

Download an object from a remote backend and store it locally:

```bash
# Fetch by SHA-256 hash (any backend)
noa storage fetch ipfs-local 2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824

# Fetch by CID (IPFS only)
noa storage fetch ipfs-local bafkreihdwdcefgh4dqkjv67uzcmw7ojel6viejvs6buyq7omaygs5sep7m
```

## How Push Works

1. **Local-first**: noa reads objects from the local `RedbObjectStore`
2. **Recursive transfer**: For each snapshot, the entire tree (blobs and
   subtrees) is walked. Objects not present on the remote are transferred.
3. **Content-addressing**: Both backends use SHA-256. For IPFS, hashes are
   converted to CIDv1 (raw codec). For S3, hashes are used as object keys.
4. **Pinning** (IPFS only): After pushing, `--pin` tells the daemon to keep
   the objects, preventing garbage collection.

## Configuration Format

```toml
# .noa/config

[[storage]]
name = "ipfs-local"
type = "ipfs"
endpoint = "http://127.0.0.1:5001"
gateway = "https://ipfs.io"
auto_pin = true

[[storage]]
name = "s3-backup"
type = "s3"
endpoint = "https://s3.example.com"
bucket = "noa-objects"
access_key = "AKIA..."
secret_key = "..."
region = "us-east-1"
```

## Programmatic Usage

```rust
use libnoa::config::StorageConfig;
use libnoa::object::{create_remote_store, ObjectStore};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cfg = StorageConfig::ipfs("local", "http://127.0.0.1:5001");
    let store = create_remote_store(&cfg).await?;

    // Store content remotely
    let blob_id = store.put_blob(b"hello world").await?;
    println!("Stored as: {}", blob_id);

    // Check existence
    assert!(store.has_blob(&blob_id).await?);

    // Retrieve
    let data = store.get_blob(&blob_id).await?;
    assert_eq!(data, b"hello world");

    Ok(())
}
```
