# Architecture

See [PLAN.md](../../PLAN.md) for the full implementation plan and architecture diagram.

## Key Concepts

- **ObjectStore**: Content-addressed blob/tree storage (SHA256)
- **AgentLog**: Per-workspace JSONL append-only log for zero-lock concurrent writes
- **Snapshot**: Immutable point-in-time tree state (analogous to Git commit)
- **Workspace**: Isolated working context for an agent (analogous to Git branch)
- **RefStore**: Named pointers to snapshots with CAS semantics
