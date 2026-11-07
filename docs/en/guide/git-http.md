# git-http Protocol

`/git/{repo}/*` serves the smart HTTP protocol: the read path drives `git upload-pack
--stateless-rpc` directly, and the write path (since W2a2) drives `git receive-pack
--stateless-rpc` directly — without the `git-http-backend` CGI wrapper, so protocol negotiation,
environment variables, and error shapes are fully under control.

## Endpoints

| Method | Path | Purpose |
|---|---|---|
| GET | `/git/{repo}/info/refs?service=git-upload-pack` | clone/fetch advertisement |
| POST | `/git/{repo}/git-upload-pack` | pack negotiation |
| GET | `/git/{repo}/info/refs?service=git-receive-pack` | push advertisement |
| POST | `/git/{repo}/git-receive-pack` | receives pushes |

`{repo}` must be a mirror job name — the serving body is that job's bare scratch clone
(`NOA_MIRROR_SCRATCH_DIR/{repo}.git`). A missing repo returns 404.

## Behavior details

- **Protocol negotiation**: the client's `Git-Protocol` header is passed through to the child
  process — this is the **git wire protocol's upstream version** (gitprotocol-v2, git's own
  standard, NOT a noa version); empty defaults to v0. This is exactly why the direct-drive
  implementation replaces git-http-backend — the CGI wrapper drops that header on the POST path,
  silently downgrading to v0 and breaking clones.
- **Advertisement preamble**: the `# service=` pkt-line (length header computed dynamically from
  the service name) + flush; RPC responses carry no preamble.
- **Content-Type**: `application/x-git-{upload,receive}-pack[-advertisement]` strictly
  distinguished.
- **Request body**: the POST body is written straight to the child process's stdin;
  `Content-Encoding: gzip` is decompressed automatically (decompression ratio > 1000:1 rejected —
  bomb protection; 50 MiB request / 200 MiB decompressed limits).
- **Timeout**: the whole exchange gets a 60s wall clock; on timeout the child process is killed
  (kill_on_drop prevents orphaned lock holders).
- **Concurrency**: a route-level semaphore bounds in-flight git exchanges (memory ceiling
  4×50 MiB).
- **Child-process environment**: after `env_clear` only `PATH` + `GIT_PROTOCOL` remain —
  environment leaks are impossible at compile time.

## Push semantics

Pushes land in the scratch's local refs; **a successful receive-pack
immediately triggers that job's import wave** (no scheduler wait) — the
pushed content is visible on the API surface right away. The wave then:

1. imports the scratch's advanced refs into noa's ref system (snapshots/workspaces);
2. when `backflow` is enabled, pushes the state back to origin.

Pushes therefore need no special hooks — the three lanes (read, receive, back-flow) share one
scratch and the closed loop converges automatically. Pushes require an authenticated identity; no
credentials 401 + `Basic` challenge, reader role 403.

## Known boundaries

- Pushes provide no deletion semantics (a delete on the noa side is not propagated to origin — a
  known asymmetry).
- dumb HTTP (raw `/info/refs` files) is not served.
- PR heads land in `refs/noa/pr/<N>` on back-flow (GitHub refuses `refs/pull/*` pushes).
- Back-flow pushes source from the scratch's git objects — **noa-native
  commits** (pure workspace snapshots that never passed through the scratch)
  are outside the back-flow scope until the snapshot → git-object serializer
  lands (see [Architecture](./architecture.md)).
