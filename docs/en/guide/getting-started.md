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

## First push (write authority)

Pushes go through the receive-pack path under `/git/{repo}` and require an authenticated identity:

```bash
cd hello
echo "hello noa" > README.md
git add -A && git commit -m "hello noa"
git push origin master
```

The push lands in the mirror scratch; the next mirror wave automatically ingests it into noa's
snapshot/ref system, and if the job has `backflow` enabled it also pushes the state back to origin
(GitHub) — the transitional shape of "noa as authority + GitHub mirror".

## Next steps

- [Architecture Overview](./architecture.md) — workspace / snapshot / ref model
- [Mirroring and Back-flow](./mirror.md) — dual lanes, PR heads, back-flow and canary
- [HTTP API reference](../reference/http-api.md) — all endpoints
