# noa — Maintenance Notes

> Created 2026-07-10 during a routine maintenance sweep.
> Note: this repo is mid-interactive-rebase (`.git/rebase-merge/` is active,
> 112 commands remaining). The notes below were added without touching any
> tracked file the rebase is operating on.

## Refresh log 2026-07-14

- **当前分支**：`dev` · 领先 `origin/dev` 0 commits · 工作区干净
- **最近提交**：`🔧 Add tokio-rustls + hyper-rustls ring pins (complete the aws-lc-rs kill).` (`ee1d951`)
- **未提交改动**：无
- **后续动作**：
  1. AI-native 分布式 VCS（per-agent workspace 隔离 + JSONL append-only log）核心路径收尾：持续验证与 entelecheia 作为 commit 协调器的握手协议，回归 import/export 与 `git` 兼容测试（`tests/compat.rs` / `tests/smoke.rs`）。
  2. 关注顶层 `patches/` 长期方案中，noa 作为协调器时的 import/export 兼容路径与上游 git 协议差异。
  3. 跨仓 `[patch]` 收敛到 `~/.cargo/config.toml`（见 `entelecheia/PLAN.md` §6 跨仓依赖约定）后，回归 tokio-rustls / hyper-rustls ring pin 的 release 构建，避免再次引入 aws-lc-rs。
- **跨仓依赖**：被 entelecheia 内部用作 commit 协调器（per-agent workspace 隔离 + JSONL append-only log）；与 shittim-chest 等 sibling 仓通过 JSONL append-only log 协议交互。

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
