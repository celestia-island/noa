# noa-server deployment runbook

How a noa master revision becomes the running `noa-server` on the DevOps
panel's backend node. The authoritative snapshot of ports, unit names, and
supervision wiring is the workspace ledger entry for port 3016
(`_ledger/ports-and-domains.md`); this runbook restates the procedure and
must be kept consistent with it — when the two disagree, the ledger wins
and this file gets fixed.

## What runs where

| Piece | Value |
|---|---|
| Service | `noa-server` (this repo, `bin/server.rs`), version = the crate's own `version` |
| Node / port | the DevOps backend node, port **3016** (bearer-token auth; binds `127.0.0.1` by default) |
| Supervision | malkuth-wrapped systemd unit `noa-server.service`; status front door = malkuth info port **8419** |
| Credentials | `NOA_API_TOKEN` only, from a `0600` env file (`/etc/noa/noa-server.env`) — the server refuses to start without it |
| Data | redb database under `/var/lib/noa` (workspaces, snapshots, PRs/approval tickets) |
| Consumers | the chest panel instances reach it via their `plugin-suite.conf` drop-in (`NOA_SERVER_URL`, `CI_GITHUB_REPOS`, `CD_TARGETS`) |

## Build

Manual compiles are frozen in this org (see the workspace build rules);
release builds come from the CI farm / build engine. Whatever lane built
it, the artifact lands in the shared archive under the org-wide naming
convention:

```
_artifacts/noa/noa-server-<crate version>-<source rev>-linux      # binary
_artifacts/noa/noa-server-<crate version>-<source rev>-linux.sha256
```

Examples already archived: `noa-server-0.4.2-64893dd-linux`,
`noa-server-0.4.2-7af872e-linux`. The version is **the `noa-server`
binary's own package version** (`env!("CARGO_PKG_VERSION")` — for this
workspace that is the root `[package] version`, e.g. `0.4.2`), not a
workspace member version guessed from a sibling crate.

## Deploy (per wave)

1. Claim the deploy-exclusive lock for this service before touching the
   deployment path (workspace `claim.py`; overlapping writers on one
   deploy path is an incident, not a race).
2. Verify the artifact: `sha256sum -c noa-server-<version>-<rev>-linux.sha256`.
3. Copy to the node's deploy path via **same-directory temp file +
   `mv -f`** (a half-written binary would look like a new version to the
   `--watch` wrapper and roll once more):
   ```bash
   install -m 0755 noa-server-<version>-<rev>-linux /srv/<deploy-path>/.noa-server.new
   mv -f /srv/<deploy-path>/.noa-server.new /srv/<deploy-path>/noa-server
   ```
   The malkuth `--watch` unit detects the change and rolls the pod — no
   manual restart; the supervision state is readable on the info front
   door (8419).
4. Post-deploy checks (the standard trio, retried through the rolling
   window — a single refused connect during the roll is not a failure):
   - deployed binary hash == archived artifact hash;
   - health: an authenticated request answers (e.g. `GET /api/v1/prs`
     with the Bearer token from the env file) — an *unauthenticated*
     request must answer 401, which is itself proof the auth gate is up;
   - the panel's DevOps source-control tab resolves (chest's
     `noa.config.get` → `configured: true`).
5. If the wave included a schema/store migration, run it explicitly and
   per-face before declaring done; noa's redb store is single-file — back
   it up (`cp --reflink=auto` to a `.bak-<ts>`) before any such wave.

## Rollback

Re-deploy the previous archived artifact (same procedure, previous
`<rev>`); the redb store tolerates a binary that is older than the file
as long as no on-disk format migration happened in between. If it did,
restore the pre-wave store backup instead — do not point a new binary at
a rolled-back store.

## PR / approval-ticket semantics (for operators)

noa PRs are the DevOps plane's approval tickets: an agent (or human)
proposes a change as a PR, a panel write-role holder **merges** (approve)
or **closes** (revoke) it. `close` is **idempotent** — re-closing an
already-closed ticket answers the unchanged closed record (200), so
panel retries and revoke reconcile loops converge instead of erroring —
and it refuses merged ones, because merged history is immutable. Every
lane below sits behind the same bearer-token gate (401 without it; the
token lives in `/etc/noa/noa-server.env`). The server API:

```
POST /api/v1/prs                     create (state open)
GET  /api/v1/prs?state=…             list (open|closed|merged)
GET  /api/v1/prs/{number}            read one
POST /api/v1/prs/{number}/merge      approve + land  (body {"squash": bool})
POST /api/v1/prs/{number}/close      revoke           (no body; idempotent)
```

Repo catalog (W2b) and the repo-scoped PR view:

```
GET    /api/v1/repos                       list  -> {"repos": [...]}
POST   /api/v1/repos                       register ({"name", "description", "default_branch"})
GET    /api/v1/repos/{name}                read one (404 when unregistered)
GET    /api/v1/repos/{name}/prs            PRs of one repo -> bare PrRecord[]
                                           (?state= must be open|closed|merged — anything else
                                            is 400, so a typo cannot read as "no open PRs")
                                           (?state=, ?base=, ?limit=, ?offset= apply;
                                            ?repo= is rejected when it contradicts the path;
                                            404 for an unregistered name — after input
                                            validation, so a bad ?limit= answers 400 first)
DELETE /api/v1/repos/{name}                remove the catalog row
                                           404 ghost → 409 while a MIRROR JOB of that name
                                           exists (delete the job first: its next upsert would
                                           re-register the row) → 409 while OPEN PRs exist
                                           (close or merge them first) → 204.
                                           Deleting the MIRROR JOB never blocks; it keeps the
                                           row while open PRs exist (or when the open-PR check
                                           fails), so the scratch clone may be gone while the
                                           row survives.
```

Read-only git serving shares the same namespace: `git clone
http://<host>:<port>/git/<repo>` (auth = API token). A clone of a name the
catalog does not know yet is auto-registered (origin `clone-served`) — best
effort: the clone succeeds even if the catalog write fails.

Pagination: the two PR list routes (`/api/v1/prs` and
`/api/v1/repos/{name}/prs`) take `?limit=` (1..1000, default 100) and
`?offset=`; the catalog/mirror/ref list routes do not paginate yet.
