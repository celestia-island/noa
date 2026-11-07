# CLI Reference

`noa` is the workspace-side command-line entry point (clap-derived; see `noa --help` for the live
signature). All commands run within a repo initialized with `noa init`.

## Command overview

| Command | Purpose |
|---|---|
| `noa init` | initialize the current directory as a noa repo (creates the `.noa` metadata directory) |
| `noa status` | current workspace state: head, uncommitted changes, workspace list |
| `noa log` | the current workspace's snapshot history (truncated rendering + parent-chain traversal) |
| `noa snapshot` | snapshot operations (create / inspect) |
| `noa workspace` | workspace management: `workspace create <name>` (per-agent isolation), switching, listing |
| `noa remote` | remote management (add / set-url / list) |
| `noa push` / `noa pull` / `noa fetch` / `noa clone` | sync with the remote (git-compatible semantics) |
| `noa resolve <path>` | resolve a nested path to a tree entry |
| `noa coauthor` | attach co-authors to commits (agent collaboration attribution) |
| `noa hook` | manage repo hooks (with a sentinel marker, safely overwriting existing hooks) |
| `noa verify` | append a `verify.run/1` record to the current workspace's agent log (JSON via stdin or `--file`) — the CLI writer for the verification ledger |
| `noa sync` | sync operations |
| `noa storage` | storage-backend operations |
| `noa pr` | PR lifecycle (forge-agnostic): `pr create` / `pr list` / `pr diff` etc. |

## The workspace-isolated workflow

```bash
noa init
noa workspace create agent-a      # agent A's isolated workspace
# … edit inside the workspace …
noa snapshot -m "agent-a's changes"
noa log                           # sees only this workspace's parent chain
```

Multiple agents each run `workspace create`, invisible to each other's heads and content — this is
the fundamental difference between noa and traditional branch workflows: the unit of isolation is
the workspace, not the ref.

## Server coordination

The CLI's push/pull/fetch/clone interoperate with the server's `/git/*` smart protocol (the same
git-compatible semantics). With the remote pointing at noa-server:

```bash
noa remote add origin http://noa.example.com:3016/git/noa
noa push origin master
```

Pushes go through receive-pack and require an authenticated identity (the token doubles as the
password; the role must have write).

## verify and the ledger

`noa verify` is the CLI writer for the verification ledger: records are appended to the current
workspace's agent log (JSONL, append-only) with the `verify.run/1` schema. Records can carry a
`hardware=<hw-id>` binding (working with the workspace-side hardware registry audit).

> The authoritative ledger lives on the file side (`/mnt/work/verify-ledger/`); CLI writes are
> fail-open dual-writes — open writers not verifying the chain is a design ruling; chain
> verification is done by readers.
