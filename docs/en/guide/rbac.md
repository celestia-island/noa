# Permissions and Users

noa's auth is "one token, two channels": REST and git share the same credential, but write
permission is enforced separately on each.

## Credentials

- **Bootstrap token**: the `NOA_API_TOKEN` environment variable. The server refuses to start with
  an empty value (empty / all-whitespace is rejected at startup); that token is the admin
  identity.
- **Registered user tokens**: created via REST (see below), stored as sha256 hashes, compared in
  constant time.

Auth shapes (identical on both channels):

```text
Authorization: Bearer <token>
Authorization: Basic <base64(any-user:token)>     # git client convention: any username
```

A server with an empty token (`NOA_API_TOKEN=""`) does not degrade into anonymous read — startup
is refused outright, and the non-empty guard before comparison adds defense in depth.

## Roles

| Role | Read | Write (REST / git push) | Admin (user CRUD) |
|---|---|---|---|
| admin | ✓ | ✓ | ✓ |
| operator | ✓ | ✓ | ✗ |
| reader | ✓ | ✗ | ✗ |

- REST: enforcement is centralized in middleware — a reader's write requests get 403; non-GET on
  `/api/v1/users` gets 403 (admin only).
- git: push (receive-pack) requires can_write — no credentials 401 (with a `Basic` challenge,
  which git clients use to prompt); credentials present but reader 403 (prompting that the write
  role is required). clone/fetch work with any authenticated identity.

## User management REST

```bash
# Create (admin)
curl -X POST -H "Authorization: Bearer $ADMIN" -H "Content-Type: application/json" \
  -d '{"name":"alice","role":"operator","token":"<alice-token>"}' \
  http://127.0.0.1:3016/api/v1/users

# List (admin) / single
curl -H "Authorization: Bearer $ADMIN" http://127.0.0.1:3016/api/v1/users
curl -H "Authorization: Bearer $ADMIN" http://127.0.0.1:3016/api/v1/users/alice

# Delete (admin)
curl -X DELETE -H "Authorization: Bearer $ADMIN" \
  http://127.0.0.1:3016/api/v1/users/alice
```

Rules:

- `name` and `role` (admin/operator/reader) are required; the server sha256-hashes the token
  before storing it.
- Updating (POST to the same endpoint with the same name) can rotate the token or change the role;
  `created_us` keeps its original value.
- Deletion is admin-only and irreversible.

## git channel example

```bash
# Read (any authenticated identity; public repos work with a placeholder username)
git clone http://<token>@noa.example.com:3016/git/noa

# Write (requires operator/admin)
git push origin master
```

An unauthenticated push gets 401 + a `Basic` challenge (git clients use it to prompt for
credentials); a push by a reader identity gets 403 (prompting that the write role is required).
