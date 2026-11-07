<p align="center"><img src="https://raw.githubusercontent.com/celestia-island/docs.celestia.world/master/res/logo/noa.webp" alt="noa" width="200" /></p>

<h1 align="center">Noa</h1>

<p align="center"><strong>AI-native distributed version control system</strong></p>

<div align="center">

[![License](https://img.shields.io/badge/license-SySL--1.0-blue.svg)](https://sysl.celestia.world)
[![GitHub](https://img.shields.io/badge/github-celestia--island%2Fnoa-blue.svg)](https://github.com/celestia-island/noa)

</div>

<div align="center">

**English** ·
[简体中文](./docs/zh-Hans/intro.md) ·
[繁體中文](./docs/zh-Hant/intro.md) ·
[日本語](./docs/ja/intro.md) ·
[한국어](./docs/ko/intro.md) ·
[Français](./docs/fr/intro.md) ·
[Español](./docs/es/intro.md) ·
[Русский](./docs/ru/intro.md) ·
[العربية](./docs/ar/intro.md)

</div>

Per-agent workspace isolation, SHA-256 content-addressed snapshots, a full git smart-HTTP
server (clone/fetch/**push**), a self-hosted forge with a built-in mirror + back-flow lane,
line-level three-way merge with exact-content rename detection, multi-user RBAC, and a CI
run-records API — all in a single binary with embedded storage.

## Documentation

The full documentation set lives in this repository (also published at
[noa.docs.celestia.world](https://noa.docs.celestia.world)). **简体中文 is the source of
truth**; other languages are synced from it.

| Document | Contents |
|---|---|
| [快速上手 / Getting Started](./docs/zh-Hans/guide/getting-started.md) | build, run the server, first clone/push |
| [架构总览 / Architecture](./docs/zh-Hans/guide/architecture.md) | workspace / snapshot / ref model |
| [镜像与回流 / Mirror & Back-flow](./docs/zh-Hans/guide/mirror.md) | dual lanes, PR heads, back-flow, canary |
| [权限与用户 / RBAC](./docs/zh-Hans/guide/rbac.md) | roles, tokens, enforcement |
| [git-http 协议 / git-http](./docs/zh-Hans/guide/git-http.md) | endpoints, push semantics |
| [合并与冲突 / Merge](./docs/zh-Hans/guide/merge.md) | diff3, rename, degradation |
| [CI 运行记录 / Runs API](./docs/zh-Hans/guide/ci-runs.md) | run records, chest integration |
| [HTTP API 参考 / API](./docs/zh-Hans/reference/http-api.md) | all REST endpoints |
| [CLI 参考 / CLI](./docs/zh-Hans/reference/cli.md) | command surface |
| [部署 / Deploy](./docs/zh-Hans/reference/deploy.md) | systemd, env, rollout |

## Quick Start

```bash
# Build
cargo build --release

# Run tests
cargo test --all-features

# Initialize a noa repository
noa init

# Create a snapshot
noa snapshot

# View the TUI
noa tui
```

For a full list of commands:

```bash
noa --help
```

## Documentation

Architecture, design, and guides live at [docs.celestia.world/en/noa](https://github.com/celestia-island/docs.celestia.world/tree/master/docs/en).

Source: [noa](https://github.com/celestia-island/noa).
