# Getting Started

This page takes you from zero to a running noa: build, start the server, and complete the first
clone / push.

## Prerequisites

- Rust stable ≥ 1.99 (installed via `rustup`)
- git ≥ 2.39 (the server's smart HTTP drives `upload-pack` / `receive-pack` directly)
- Optional: node + pnpm (to build the WebUI)

## Build

```bash
git clone https://github.com/celestia-island/noa.git
cd noa
cargo build --release --locked
```

Artifacts: `target/release/noa` (CLI) and `target/release/noa-server` (server).

## Start the server

The minimal configuration needs only one API token (the bootstrap admin credential):

```bash
export NOA_API_TOKEN="<your-token>"
export NOA_MIRROR_SCRATCH_DIR=/var/lib/noa/mirror-cache   # mirror scratch root
export NOA_MIRROR_ENABLED=1                               # enable mirror scheduling (default 900s)
export NOA_MIRROR_GH_TOKEN="<github-pat>"                 # optional: back-flow / canary push credential
noa-server --host 0.0.0.0 --port 3016
```

After startup:

```bash
curl -H "Authorization: Bearer <your-token>" http://127.0.0.1:3016/api/v1/repos
```

A response of `[]` means the service is ready. All REST endpoints require bearer auth; the same
goes for the read paths under `/git/{repo}/*` (unauthenticated requests get a 401 with a `Basic`
challenge).

## The noa-native workflow (recommended)

The `noa` CLI is the first-class entry — a clone is a repo where `.git` and `.noa` coexist (git manages the source, noa manages the agent data):

```bash
noa clone http://<token>@127.0.0.1:3016/git/hello --path hello
cd hello
noa workspace create agent-a     # a per-agent isolated workspace
# … edit files …
noa snapshot create -m "change" -a agent-a
noa log                          # this workspace's snapshot history
noa push                         # publishes the workspace snapshot (serialized to a git commit)
```

`noa push` publishes the **workspace snapshot state** (it creates a `[noa export]` commit) — not the `.git` commits. The two lanes are parallel; don't mix them (run `noa snapshot create` before pushing, or you publish a stale snapshot). Authentication injects from the `NOA_GIT_USER` + `NOA_GIT_PASS` environment variables. The whole directory (including `.git/noa`) copies to another machine and keeps working.

## The git compatibility lane (any git tool works)

The same `/git/{repo}` endpoint also speaks standard git smart HTTP — stock git, CI, farm work unmodified:

```bash
git clone http://<token>@127.0.0.1:3016/git/hello
```

Pushes (receive-pack) require the write role; a landed push **immediately triggers the import wave** (back-flow follows on the same wave). This is the transition form that keeps farm/CNB at zero change.

## First mirror (ingesting a GitHub repo into noa)

```bash
curl -X POST -H "Authorization: Bearer <your-token>" \
  -H "Content-Type: application/json" \
  -d '{
    "name": "hello",
    "remote_url": "https://github.com/you/hello.git",
    "github_slug": "you/hello",
    "branches": ["master"],
    "tags": true,
    "pr_heads": true,
    "enabled": true
  }' \
  http://127.0.0.1:3016/api/v1/mirror/jobs

curl -X POST -H "Authorization: Bearer <your-token>" \
  -H "Content-Type: application/json" \
  -d '{"job":"hello"}' \
  http://127.0.0.1:3016/api/v1/mirror/run
```

Once the wave finishes, the repo is cloneable through noa:

```bash
git clone http://<your-token>@127.0.0.1:3016/git/hello
```

## Backup & migration

The whole server state exports into one portable directory (see
[Deployment](../reference/deploy.md) § backup): a consistent redb snapshot +
one cloneable git bundle per repo + a sha256 manifest; `noa-server
--import-state <dir>` restores offline. The acceptance gate is the drill:
export → restore on another machine → serve → reconcile.


## Next steps

- [Architecture Overview](./architecture.md) — workspace / snapshot / ref model
- [Mirroring and Back-flow](./mirror.md) — dual lanes, PR heads, back-flow and canary
- [HTTP API reference](../reference/http-api.md) — all endpoints
