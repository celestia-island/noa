# noa — Maintenance Notes

> Created 2026-07-10 during a routine maintenance sweep.
> Note: this repo is mid-interactive-rebase (`.git/rebase-merge/` is active,
> 112 commands remaining). The notes below were added without touching any
> tracked file the rebase is operating on.

## Open issue: license metadata mismatch (needs maintainer decision)

The repository ships a **Synthetic Source License (SySL) 1.0** text in the
root `LICENSE` file, but the crate declares the SPDX license as
**Apache-2.0** in `Cargo.toml`:

| File | Declared license |
|------|------------------|
| `LICENSE` (file text) | SySL 1.0 |
| `Cargo.toml` | `license = "Apache-2.0"` |
| `README.md` badge | Apache-2.0 |

This means crates.io metadata advertises Apache-2.0 while the actual
license file is SySL — a real conflict for downstream users relying on the
SPDX expression.

### Why this was not auto-fixed

1. SySL 1.0 is not a standard SPDX identifier, so crates.io does not accept
   it as `license = "SySL-1.0"`. Sibling crates (hikari, kirino, lagrange,
   malkuth, seia, shirabe, tairitsu, yuuka) use `license-file = "LICENSE"`.
2. noa is mid-interactive-rebase, so editing `Cargo.toml`/`README.md` now
   risks a rebase conflict.

### Resolution decided (2026-07-10)

Decision: option 2 — change Cargo.toml to license-file = "LICENSE" to match
the actual SySL file (mirrors hikari/kirino/lagrange/hifumi/arona).

NOT YET APPLIED to noa because the repo is mid-interactive-rebase and
Cargo.toml may be in the rebased history. Apply this one-line change
(license = "Apache-2.0" -> license-file = "LICENSE") and republish with a
semver bump once the rebase completes.

### Original suggested resolution (pick one, after the rebase completes)

1. **The crate really is Apache-2.0**: replace the root `LICENSE` with the
   Apache-2.0 text and keep `license = "Apache-2.0"` (align the file with
   the metadata).
2. **The crate is SySL**: change `Cargo.toml` to
   `license-file = "LICENSE"` (drop `license = "Apache-2.0"`), update the
   README badge to SySL, and bump the crate version — mirroring hikari.

(See hifumi/PLAN.md and arona/PLAN.md for the same issue in those repos.)

## Done during this sweep

- Unified the Chinese docs locale codes (`zhs`/`zht` → `zh-hans`/`zh-hant`):
  renamed the stale mdBook stubs into the real locale dirs, repointed
  lagrange.toml's language order and every intro.md language switcher, and
  dropped the redundant zhs/zht directories.
