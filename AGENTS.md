# AGENTS.md — noa

Instructions for agents (human or automated) working on this repository from
the Celestia workspace.

## Binary build / overwrite-install serialization

Multiple agents work on this repository in parallel, and any of them may
compile the `noa` binary and overwrite the locally installed one. Those
operations must be serialized through the workspace's optimistic claim system
(`claim.py`, exposed at the workspace root as a shared tool):

- **Acquire before any compile + overwrite-install of the `noa` binary**:

  ```
  claim.py acquire noa bin-build --owner "<agent>" --auto-renew --note "<version + reason>"
  ```

  The token returned by `acquire` is the only credential for `renew` /
  `release`. Always `release` when done — success and abandonment alike.

- **Conflict resolution when the claim is held** (user ruling, 2026-11-03):

  1. **Flow downstream (preferred)**: if the build is not urgent, do not
     compile at all — wait for the holder to finish installing and use the
     new version directly (saves one compile).
  2. **Wait + expire-takeover**: if the build blocks your delivery, wait for
     the claim TTL (default 2h) to lapse and acquire naturally; a holder with
     a live `--auto-renew` heartbeat is never preempted.
  3. **Never `--force`** (exit 6, token mismatch) unless `claim.py list`
     proves the holder's heartbeat is dead.

- **Installs must be atomic**: build to `<bin>.new` first, then rename it
  into place. Never overwrite a binary that another agent may be executing
  mid-write.

- The claim ledger in `claim.py list` (with heartbeat timestamps) is the
  single source of truth for who is building; do not duplicate it elsewhere.

Background: the verify-ledger initiative makes frequent local rebuilds of
`noa` the norm (log-carried `verify.run/1` records), which is why the
serialization rule exists. The claim system itself lives in the workspace,
not in this repository.
