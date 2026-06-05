# Agent Log Design

## Overview

The AgentLog is noa's high-throughput write layer. It provides append-only
JSONL files for each workspace, enabling zero-lock concurrent writes from
multiple AI agents.

## Log Entry Format

Each line is a JSON object:

```jsonl
{"seq":1,"op":"write","path":"src/main.rs","blob":"a1b2c3...","ts":1717592400000000}
{"seq":2,"op":"delete","path":"src/old.rs","ts":1717592401000000}
{"seq":3,"op":"rename","from":"src/foo.rs","to":"src/bar.rs","ts":1717592402000000}
{"seq":4,"op":"snapshot","snapshot_id":"noa_z7x9","parent":"noa_y6w8","message":"feat","ts":1717592405000000}
{"seq":5,"op":"merge","from_workspace":"feature-1","from_snapshot":"noa_abc","base":"noa_def","ts":1717592408000000}
```

### Fields

| Field | Type | Description |
|-------|------|-------------|
| `seq` | u64 | Monotonic sequence number per workspace |
| `op` | string | Operation type: write, delete, rename, snapshot, merge |
| `path` | string | Target file path (write, delete) |
| `blob` | string | Blob hash (write) |
| `from` | string | Source path (rename) |
| `to` | string | Destination path (rename) |
| `ts` | u64 | Microsecond-precision Unix timestamp |

## File Structure

```
.nao/agent-logs/
├── default.log       # workspace "default"
├── feature-1.log     # workspace "feature-1"
├── agent-001.log     # workspace "agent-001"
└── ...
```

Each workspace gets exactly one log file. File name matches workspace name.

## Write Path

```rust
async fn append(&self, workspace: &str, entry: &LogEntry) -> Result<()> {
    let file = self.get_or_create_file(workspace)?;
    let line = serde_json::to_string(entry)? + "\n";
    file.write_all(line.as_bytes())?;
    file.sync_data()?;  // fdatasync for durability
    Ok(())
}
```

Key properties:
- **O_APPEND**: Kernel guarantees atomic appends
- **fsync per write**: Ensures durability after crash
- **One fd per workspace**: Cached in memory for performance

## Read Path

```rust
async fn read_all(&self, workspace: &str) -> Result<Vec<LogEntry>> {
    let path = self.log_dir.join(format!("{}.log", workspace));
    let content = tokio::fs::read_to_string(&path).await?;
    content.lines()
        .filter(|l| !l.is_empty())
        .map(|l| serde_json::from_str(l))
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| NoaError::Serialization(e.to_string()))
}
```

## Snapshot Computation

The `SnapshotEngine` replays log entries to build a tree:

```
1. Read all entries for workspace
2. Start from parent snapshot's tree (or empty)
3. For each entry (sorted by seq):
   - write:  tree[path] = blob_hash
   - delete: tree.remove(path)
   - rename: tree[to] = tree.remove(from)
4. Store resulting tree → ObjectStore
5. Create snapshot with tree hash
```

## Consolidation

When multiple agent logs need merging:

```
1. Read all logs: agent-001.log, agent-002.log, ...
2. Flatten into single list
3. Sort by timestamp (µs precision)
4. Replay in order against base tree
5. Create unified snapshot
```

## Comparison: Why Not...

### SQLite for agent logs?

- **Write amplification**: SQLite B-tree updates for sequential appends
- **Locking**: SQLite uses WAL locks (single writer)
- **fsync overhead**: SQLite issues multiple fsyncs per transaction
- **Overkill**: Agent logs are append-only — no random reads or updates

### redb for agent logs?

- **Single writer**: redb's MVCC requires a write transaction
- **Contention**: Multiple agents writing to same DB → serialized
- **Not append-optimized**: redb is a general-purpose KV store

### In-memory buffer?

- **Durability**: Process crash loses all buffered writes
- **Memory pressure**: 100 agents × 1000 writes = 100K entries in memory
- **Complexity**: Requires background flush thread with crash recovery

### Plain JSONL with O_APPEND?

✅ This is what noa uses:
- **Minimal overhead**: One write + one fsync per entry
- **Kernel-guaranteed atomicity**: O_APPEND on POSIX
- **Crash recovery**: Only last entry may be partial (detect by trailing newline)
- **Human-readable**: JSONL is inspectable with standard tools
- **Zero lock contention**: One file per workspace

## Performance

Benchmark (ext4, SSD, Linux):

| Metric | Value |
|--------|-------|
| Single write latency | ~0.05ms (append + fdatasync) |
| Throughput (1 workspace) | ~20,000 writes/sec |
| Throughput (100 workspaces) | ~10,000+ writes/sec |
| File size per 1M entries | ~200MB (average 200 bytes/entry) |

## Crash Recovery

On startup, scan each log file:
1. Read all complete lines (ending with `\n`)
2. Discard last line if truncated (incomplete write)
3. Verify `seq` is monotonically increasing
4. Rebuild in-memory state from valid entries

This ensures no partial or corrupted entries are used for snapshot computation.
