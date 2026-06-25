# CLI Reference

## `noa init [path]`

Initialize a new `.noa/` repository. Creates `noa.redb`, `agent-logs/`, `HEAD`, and `config`.

```bash
noa init .           # current directory
noa init /path/repo  # specific path
```

## `noa status`

Show current workspace and head snapshot.

```bash
noa status
# On workspace: default (head: noa_abc123, msg: initial)
```

## `noa log [options]`

View snapshot history.

| Flag | Default | Description |
|------|---------|-------------|
| `-w, --workspace` | current HEAD | Filter by workspace |
| `-l, --limit` | 20 | Max entries to show |

```bash
noa log
noa log --workspace feature-1 --limit 50
```

## `noa snapshot <subcommand>`

### `noa snapshot create [-m msg] [-a author]`

Create a snapshot from the current workspace's agent log.

```bash
noa snapshot create -m "add login feature" -a "agent-001"
```

### `noa snapshot list`

List all snapshots across workspaces.

### `noa snapshot diff <a> <b>`

Show file-level differences between two snapshots.

```bash
noa snapshot diff noa_abc123 noa_def456
```

## `noa workspace <subcommand>`

### `noa workspace create <name> [--agent <id>]`

Create a new workspace forked from the current HEAD.

### `noa workspace switch <name>`

Switch the active workspace (updates HEAD).

### `noa workspace list`

List all workspaces. `*` marks the active one.

### `noa workspace delete <name>`

Delete a workspace (cannot delete the active workspace).

### `noa workspace merge <from>`

Merge another workspace into the current one using three-way merge.

```bash
noa workspace switch default
noa workspace merge feature-1
```

## `noa remote <subcommand>`

### `noa remote add <name> <url>`

Add a remote repository.

### `noa remote remove <name>`

Remove a remote.

### `noa remote list`

List all configured remotes.

## `noa push [--remote name]`

Push to a remote (not yet implemented).

## `noa pull [--remote name]`

Pull from a remote (not yet implemented).

## `noa fetch [--remote name]`

Fetch from a remote without merging (not yet implemented).

## `noa clone <url> [path]`

Clone a remote repository (not yet implemented).

## `noa storage <subcommand>`

Manage remote storage backends (IPFS, S3/MinIO) for object distribution.

### `noa storage add <name> --type <backend> [options]`

Add a remote storage backend.

```bash
# IPFS backend
noa storage add ipfs-local --type ipfs
noa storage add ipfs-local --type ipfs --endpoint http://192.168.1.100:5001 --auto-pin

# S3 / MinIO backend
noa storage add s3-backup --type s3 --endpoint https://s3.example.com \
  --bucket noa-objects --access-key AK... --secret-key ...

# FTP backend
noa storage add ftp-server --type ftp --endpoint ftp.example.com \
  --username noa --password secret

# FTPS (FTP over TLS)
noa storage add ftps-secure --type ftp --endpoint ftp.example.com \
  --username noa --password secret --tls
```

| Flag | Applies to | Default | Description |
|------|-----------|---------|-------------|
| `-t, --type` | all | required | Backend type: `ipfs`, `s3`, or `ftp` |
| `--endpoint` | all | `http://127.0.0.1:5001` (ipfs) | API endpoint URL |
| `--gateway` | ipfs | `https://ipfs.io` | Public IPFS gateway URL |
| `--auth-token` | ipfs | none | Bearer token for pinning services |
| `--auto-pin` | ipfs | `false` | Auto-pin pushed objects |
| `--bucket` | s3 | required | S3 bucket name |
| `--access-key` | s3 | required | S3 access key |
| `--secret-key` | s3 | required | S3 secret key |
| `--region` | s3 | `us-east-1` | S3 region |
| `--username` | ftp | required | FTP username |
| `--password` | ftp | required | FTP password |
| `--port` | ftp | `21` | FTP port |
| `--tls` | ftp | `false` | Use FTPS (explicit TLS) |

### `noa storage remove <name>`

Remove a configured storage backend.

### `noa storage list`

List all configured storage backends.

### `noa storage status [name]`

Show connection status for all or a specific backend.

### `noa storage push [options]`

Push snapshots to a remote storage backend.

| Flag | Default | Description |
|------|---------|-------------|
| `--target <name>` | auto-pin backends | Specific backend to push to |
| `--snapshot <id>` | all | Push a specific snapshot |
| `-w, --workspace <name>` | all | Push snapshots from a workspace |
| `--pin` | false | Pin objects after push (IPFS only) |

```bash
noa storage push --target ipfs-local --pin
noa storage push --target s3-backup --snapshot noa_abc123
```

### `noa storage fetch <target> <hash-or-cid>`

Fetch an object from a remote backend by SHA-256 hash or CID.

```bash
noa storage fetch ipfs-local bafkreih...
noa storage fetch s3-backup 2cf24dba5fb0a30e...
```
