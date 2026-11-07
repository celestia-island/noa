# Architecture Overview

noa is an agent-native distributed version control system. It is not "a wrapper around git": the
core object model is SHA-256 content-addressed out of the box, while staying byte-for-byte
compatible with git's object algebra (the same file content hashes to the same blob OID in both
noa and git — mirror fidelity verification relies on exactly this).

## Three-layer model

```
Workspace          — one isolated workspace per agent, invisible to each other
   │ head points to
Snapshot           — content-addressed commit (SHA-256), DAG structure (multiple parents = merge)
   │ tree_hash points to
ObjectStore        — blob / tree / symlink / gitlink, git object-algebra compatible
```

- **Workspace** is noa's core isolation primitive: multiple agents each hold their own workspace
  and head inside the same repo without overwriting each other. System namespaces such as
  `mirror.<job>.<branch>` also exist in workspace form.
- **Snapshot** is the equivalent of a commit: `tree_hash` + `parents[]` + `author` + `timestamp` +
  `message`. Snapshot IDs look like `noa_<sha256>` (content-addressed; repeated imports of the
  same tree hash are idempotent).
- **Refs** (RedbRefStore): `HEAD`, `mirror/{job}/{branch}`, `mirror/{job}/tags/{tag}`,
  `mirror/{job}/pr/{N}` and so on are all ref rows in redb pointing at snapshot IDs.

The storage engine is a single-file embedded KV store (redb) — deployment means copying a
directory; no external database required.

## Server components (noa-server)

| Component | Responsibility |
|---|---|
| REST API (`/api/v1/*`) | CRUD for repos / PRs / runs / users / mirror-jobs |
| git-http (`/git/{repo}/*`) | smart HTTP: upload-pack (clone/fetch) + receive-pack (push, since W2a2) |
| Mirror engine | scheduled waves: fetch master / tags / PR heads from origin and import into noa; can back-flow push |
| run-records | CI run-record storage (the data source for the chest DevOps panel) |
| Auth middleware | route-level bearer auth + role enforcement (REST) and can_write enforcement (git push) |

All server state lives in one redb database; the server is stateless down to "the directory is
everything".

## git object compatibility

Both import and back-flow assume the git object algebra stays invariant:

- blob: the raw content is the git blob body (`git_blob_oid(content)` matches git);
- tree: imports are cross-checked item by item by path/kind/oid (fidelity verification);
- commit: the parent chain is maintained by mirror waves (`noa_<sha>` content addressing + parent
  links).

Back-flow pushes from noa → git therefore need no conversion layer: the git objects in the scratch
clone map one-to-one onto noa objects.

## The role of the mirror scratch

`NOA_MIRROR_SCRATCH_DIR/{job}.git` is a bare clone per mirror job with a triple role:

1. **Fetch-source cache** — waves fetch from origin; the importer reads git objects from here;
2. **Serving body for git-http** — `/git/{repo}` runs upload-pack / receive-pack directly against
   it;
3. **Push source for back-flow** — the back-flow lane pushes its local refs back to origin
   atomically.

A push (receive-pack) updates the scratch's local refs; the next wave imports them into noa's ref
system, and the back-flow lane pushes them back to origin — the three paths share one scratch, and
the loop closes.

## Directory index

- [Getting Started](./getting-started.md)
- [Mirroring and Back-flow](./mirror.md)
- [Permissions and Users](./rbac.md)
- [git-http protocol](./git-http.md)
- [Merge and conflicts](./merge.md)
- [CI run records](./ci-runs.md)
- [HTTP API reference](../reference/http-api.md)
- [CLI reference](../reference/cli.md)
- [Deployment](../reference/deploy.md)
