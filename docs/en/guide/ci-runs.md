# CI Run Records (runs API)

noa has built-in CI run-record storage: one "pipeline of checks executed against some repo at some
head" is one run. This is the data source for the `CI_BACKEND=noa` lane of the chest DevOps
panel's CI view.

## Endpoints

| Method | Path | Description |
|---|---|---|
| POST | `/api/v1/repos/{name}/runs` | records one run (write role) |
| GET | `/api/v1/repos/{name}/runs?limit=&offset=` | list, newest first |

- `{name}` must exist in the catalog (otherwise 404);
- `number` is assigned by the server per repo (atomic: scan and write happen in the same write
  transaction, so concurrent records never overwrite each other's numbers);
- Validation: `head_sha` is 40 hex chars; `status` ∈
  `queued | running | success | failure | cancelled`; `kind` is 1..=64 characters; `actor` is
  1..=200; `log_ref` / `summary` ≤ 2000.

## Recording a run

```bash
curl -X POST -H "Authorization: Bearer $TOKEN" -H "Content-Type: application/json" \
  -d '{
    "head_sha": "<40-hex>",
    "kind": "basic-check",
    "status": "success",
    "actor": "farm-runner-1",
    "log_ref": "https://ci.example.com/logs/123"
  }' \
  http://127.0.0.1:3016/api/v1/repos/noa/runs
```

The response (201): the server fills in `repo`, `number`, and `started_us` (when the client did
not provide it).

## Query

```bash
curl -H "Authorization: Bearer $TOKEN" \
  "http://127.0.0.1:3016/api/v1/repos/noa/runs?limit=10&offset=0"
```

Returned rows look like:

```json
{
  "repo": "noa",
  "number": 1,
  "head_sha": "…",
  "kind": "basic-check",
  "status": "success",
  "actor": "farm-runner-1",
  "started_us": 1794049465562618
}
```

## Design boundaries

- **Logs are not stored in noa**: records carry an opaque `log_ref` pointer to the log store of
  the dispatch pipeline;
- **Row content has limits** (see validation) — preventing authenticated writers from bloating row
  size;
- **Storage**: the redb `runs` table, keyed `{repo}#{number:020}` (lexicographic order =
  in-repo time order).

## chest integration

chest's `ci_ops` handler reads the `CI_BACKEND` environment variable:

- `github` (default): the v1 GitHub Actions lane, behavior identical to historical versions;
- `noa`: reads the noa runs API pointed to by `NOA_API_BASE` + `NOA_API_TOKEN`, normalizing rows
  into the same panel shape as GitHub rows (`kind`→name, `number`→run_number,
  `started_us`→RFC3339 `created_at`, terminal statuses mapped to conclusion); the watch list uses
  `CI_NOA_REPOS` (single-segment directory names), falling back to the last segment of
  `CI_GITHUB_REPOS` when unset.

The panel's `ci.config.get` reports the `backend` field; `configured` is backend-aware — a
pure-noa deployment (no GitHub token) counts as connected as long as the noa triple is complete.
