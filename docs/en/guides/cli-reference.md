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
