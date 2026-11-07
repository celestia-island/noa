# Deployment

The deployment shape of noa-server: single binary + single redb database + a systemd unit.
Production instances are overseen by malkuth.

## The deployment trio

1. **Artifact**: the output of `cargo build --release --locked --bin noa-server` is copied to the
   deploy path (atomic replace: write `<bin>.new` first, then rename), with sha256 verified on
   both sides;
2. **Health retry**: after startup, poll `/api/v1/*` (401 = process alive and auth working);
3. **Rollback**: keep the previous artifact path; `mv` back + restart.

## Environment variables (Class-A bootstrap config → 0600 env file)

| Variable | Description |
|---|---|
| `NOA_API_TOKEN` | bootstrap admin token; **empty / all-whitespace refuses startup** |
| `NOA_MIRROR_ENABLED` | mirror scheduling switch |
| `NOA_MIRROR_INTERVAL_SECS` | wave interval (default 900) |
| `NOA_MIRROR_SCRATCH_DIR` | scratch root (e.g. `/var/lib/noa/mirror-cache`) |
| `NOA_MIRROR_GH_TOKEN` | GitHub write credential for back-flow / canary (the default `token_env` name) |
| `NOA_MIRROR_GH_TOKEN_ENV` | overrides the env-var name holding the credential (defaults to the one above) |

Credentials only enter the deployment surface (0600 env / systemd LoadCredential), never the DB,
the git tree, or docs.

## systemd unit

```ini
[Service]
EnvironmentFile=/etc/noa/noa-server.env
ExecStart=/usr/local/bin/noa-server --host 0.0.0.0 --port 3016
Restart=on-failure
```

- Listens on `0.0.0.0` (family-node convention: bearer auth on all routes + write-role
  enforcement, a safe exposure surface);
- After changing env: `systemctl daemon-reload && systemctl restart`; verify that
  `tr '\0' '\n' < /proc/$MAINPID/environ` contains the new keys;
- Production instances are overseen by malkuth (acquire the deployment-exclusivity claim before
  touching deploy paths).

## Ports and domains

Port allocation lives in the workspace ledger (`_ledger/ports-and-domains.md`); the production
instance is 3016, with the nginx front door doing TLS termination on top.

## Multiple instances

redb is a single-file store — one instance, one database file. Multiple instances = multiple
directories, multiple ports (e.g. the `chest-demo` and production-chest dual-instance mode);
cross-instance config transport goes through RPC import / export, never config-file copies.

## Post-deploy verification

```bash
# Auth and readiness
curl -s -o /dev/null -w '%{http_code}\n' http://127.0.0.1:3016/api/v1/prs   # expect 401
# Mirror wave
curl -X POST -H "Authorization: Bearer $T" -H "Content-Type: application/json" \
  -d '{"job":"noa"}' http://127.0.0.1:3016/api/v1/mirror/run               # expect 202
```

Mirror details and back-flow semantics: see [Mirroring and Back-flow](../guide/mirror.md).
