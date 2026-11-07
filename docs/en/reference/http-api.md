# HTTP API Reference

Base URL: `http://<host>:<port>/api/v1`. All endpoints require bearer auth (`Authorization: Bearer
<token>`); unauthenticated requests get 401 + a `Basic` challenge. Role enforcement: a reader's
write requests get 403; `/users` non-GET is admin-only.

Errors are uniformly shaped `{"error": "..."}`; pagination is inline (`?limit=` 1..=1000,
`?offset=`).

## repos — repo catalog

| Method | Path | Description |
|---|---|---|
| GET | `/repos` | list the catalog |
| POST | `/repos` | register (`name` required, same shape as mirror job names) |
| GET | `/repos/{name}` | single |
| DELETE | `/repos/{name}` | delete (409 while a mirror job is alive or open PRs exist) |
| GET | `/repos/{name}/prs` | repo-scoped PR list |
| POST/GET | `/repos/{name}/runs` | CI run records (see [CI run records](../guide/ci-runs.md)) |

## prs — pull requests

| Method | Path | Description |
|---|---|---|
| GET | `/prs?state=&base=&repo=&limit=&offset=` | global list (`state` ∈ open/closed/merged) |
| POST | `/prs` | create (title, repo, base, head, etc.) |
| GET | `/prs/{number}` | single |
| POST | `/prs/{number}/merge` | merge (squash/fast-forward; conflicts 409 with a conflict list) |

Repo-scoped variant: `GET /repos/{name}/prs` — the path decides the repo; a `?repo=` filter that
contradicts it returns 400.

## users — users and roles (admin)

| Method | Path | Description |
|---|---|---|
| GET | `/users` | list (admin) |
| POST | `/users` | create/update (same name = rotate token or change role; `created_us` preserved) |
| GET | `/users/{name}` | single (admin) |
| DELETE | `/users/{name}` | delete (admin, irreversible) |

Roles: `admin` (everything) / `operator` (read-write) / `reader` (read-only). Fields and
validation: see [Permissions and Users](../guide/rbac.md).

## mirror — mirror jobs

| Method | Path | Description |
|---|---|---|
| GET | `/mirror/jobs` | list all jobs (including `last_status` / `last_lane` / `last_detail`) |
| POST | `/mirror/jobs` | create/update (same-name validation resets the job in full) |
| DELETE | `/mirror/jobs/{name}` | delete a job |
| POST | `/mirror/run` | manual trigger (body `{"job":"<name>"}`, 202 async) |

Fields, validation, and lane semantics: see [Mirroring and Back-flow](../guide/mirror.md).

## git-http — smart protocol

`/git/{repo}/*`: clone/fetch (upload-pack) and push (receive-pack). Auth, behavior details, and
boundaries: see [git-http protocol](../guide/git-http.md).

## Common conventions

- `limit` defaults to 50 and caps at 1000 on all list endpoints (`limit=0` → 400);
- pagination parameters are **inline** in the query string (no nested wrapper object);
- server-assigned fields (such as `number`, `started_us`) are set by the server; client-provided
  values are overwritten;
- errors do not leak internal details (the constant `internal server error` shape); details go to
  server logs.
