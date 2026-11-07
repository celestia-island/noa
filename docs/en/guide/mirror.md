# Mirroring and Back-flow

The mirror system ingests a repo's master, all tags, and active PR heads from external hosts
(GitHub etc.) into noa; the back-flow lane then pushes noa's state back to origin — forming the
transitional "noa as authority + GitHub mirror" shape, while farm/CNB keep fetching from GitHub,
unchanged.

## Mirror jobs (MirrorJob)

```json
{
  "name": "noa",
  "remote_url": "https://github.com/celestia-island/noa.git",
  "github_slug": "celestia-island/noa",
  "branches": ["master"],
  "tags": true,
  "pr_heads": true,
  "backflow": true,
  "canary_push": true,
  "enabled": true
}
```

| Field | Description |
|---|---|
| `name` | `^[a-z0-9][a-z0-9-]{0,63}$`; also the scratch directory name, the catalog repo name, and the clone path segment |
| `remote_url` | only `https://` / `file://` allowed; embedded credentials (`@`, `?`) rejected |
| `github_slug` | `owner/repo` shape check; used by the Git Data API lane (tags / PR-head pulls) and the canary write-probe; the back-flow push goes through `remote_url` |
| `branches` | at least one; `tags`/`tags/*`, `pr/*` (PR namespace conflict), and invalid shapes of derived workspaces are all rejected |
| `tags` | mirrors all tags |
| `pr_heads` | mirrors active PR heads (`refs/pull/<N>/head` → `mirror/{job}/pr/<N>`) |
| `backflow` | pushes the scratch state back to origin every wave (details below) |
| `canary_push` | pushes one probe ref to verify the write direction |
| `enabled` | when off, the scheduler skips the job |

CRUD: `POST/GET /api/v1/mirror/jobs`, `DELETE /api/v1/mirror/jobs/{name}`,
`POST /api/v1/mirror/run` (manual trigger, body `{"job":"<name>"}`).

## Dual lanes

- **git lane**: bare scratch clone + `git fetch` — tags, PR heads, and back-flow pushes all go
  through it.
- **API lane**: when the git transport dies (the network-jitter family), automatically degrades to
  per-blob import via the Git Data API; every blob's SHA is verified (fidelity does not degrade
  with the lane).

Degradation is per wave: `ensure_scratch` fails → this wave takes the API lane → self-heal deletes
the half-broken scratch and the next wave re-clones. Back-flow pushes run only while the git lane
is available (the API cannot push refs).

## PR heads

With `pr_heads: true`, each wave additionally fetches `refs/pull/<N>/head` and imports it as
`mirror/{job}/pr/<N>` + a workspace of the same name. The PR namespace and the branch namespace
(branches shaped `pr/<x>`) are mutually exclusive and rejected on upsert.

## Back-flow

```text
scratch local refs ──atomic push──▶ origin
  +refs/heads/*:refs/heads/*
  +refs/tags/*:refs/tags/*
  +refs/pull-heads/*:refs/noa/pr/*     (only when pull-heads exist in the scratch)
```

- **Credentials**: the token resolved from `token_env` (default `NOA_MIRROR_GH_TOKEN`) is embedded
  into a one-shot push URL as `x-access-token:<token>`; the token never lands in the scratch
  config.
- **Force semantics**: after the flip, noa is the write authority and origin's refs converge onto
  the scratch. Asymmetric deletion (a delete on the noa side does not delete the origin-side ref)
  is a known limitation.
- **Failure semantics**: a failed back-flow marks the wave FAILED — this is an acceptance-critical
  lane and must stay conspicuous.
- **Namespace bridging**: GitHub refuses pushes to `refs/pull/*`, so PR heads land in
  `refs/noa/pr/<N>`.

## Canary probe

With `canary_push: true`, every wave pushes a `noa-mirror-canary` probe ref to origin (the commit
of the first replicated branch), first via git transport and degrading to the Git Data API on
failure — continuously verifying the write-direction credentials and connectivity. A failed probe
makes the wave fail loudly.

## Fidelity verification

Every imported commit is cross-checked item by item: the path set, entry kind, and blob OID
(`git_blob_oid` matches git) must match exactly; any difference fails that item and lands in the
wave's error details. Back-flow depends on this invariant — noa objects map one-to-one onto git
objects, so pushes need no conversion layer.

## Stale-ref cleanup (reconcile)

The upstream sets of tags and PR heads form the reconcile set: refs that no longer appear upstream
are cleaned out of noa (the `mirror/{job}/tags/` and `mirror/{job}/pr/` namespaces). The
back-flow tags push likewise follows upstream (a delete on the noa side does not delete the origin
side — a known asymmetry).

## Scheduling

With `NOA_MIRROR_ENABLED=1`, the scheduler runs one round over all enabled jobs every 900 seconds;
`POST /mirror/run` triggers one manually. Wave details (lane, per-item results, backflow/canary
status) are on `GET /api/v1/mirror/jobs` under `last_detail` / `last_lane`.
