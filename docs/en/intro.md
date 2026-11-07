<p align="center"><img src="https://raw.githubusercontent.com/celestia-island/docs.celestia.world/dev/res/logo/noa.webp" alt="noa" width="200" /></p>

<h1 align="center">Noa</h1>

<p align="center"><strong>The agent-native distributed version control system</strong></p>

<div align="center">

[![License](https://img.shields.io/badge/license-SySL%201.0-blue)](../../LICENSE)
[![GitHub](https://img.shields.io/badge/github-celestia--island%2Fnoa-blue.svg)](https://github.com/celestia-island/noa)

</div>

<div align="center">

**English** ·
[简体中文](../zh-Hans/intro.md) ·
[繁體中文](../zh-Hant/intro.md) ·
[日本語](../ja/intro.md) ·
[한국어](../ko/intro.md) ·
[Français](../fr/intro.md) ·
[Español](../es/intro.md) ·
[Русский](../ru/intro.md) ·
[العربية](../ar/intro.md)

</div>

Per-agent workspace isolation, SHA-256 content-addressed snapshots, a full git smart-protocol server
(clone/fetch/push), a self-hosted forge with built-in mirroring and back-flow, line-level three-way
merge with exact rename detection, multi-user RBAC, a CI run-records API — all built into a single
binary + embedded storage.

## Documentation

The complete English documentation lives in this repo at `docs/en/` (this site mirrors it at
[noa.docs.celestia.world](https://noa.docs.celestia.world)):

| Docs | Contents |
|---|---|
| [Getting Started](./guide/getting-started.md) | Building, starting the server, the first clone/push |
| [Architecture Overview](./guide/architecture.md) | Workspace / snapshot / ref model, server components |
| [Mirroring and Back-flow](./guide/mirror.md) | Dual lanes, PR heads, back-flow and canary, fidelity verification |
| [Permissions and Users](./guide/rbac.md) | Roles, tokens, enforcement across the REST and git channels |
| [git-http protocol](./guide/git-http.md) | Smart-protocol endpoints, push semantics, protocol negotiation |
| [Merge and conflicts](./guide/merge.md) | Line-level diff3, exact rename, degradation semantics |
| [CI run records](./guide/ci-runs.md) | The runs API, chest integration |
| [HTTP API reference](./reference/http-api.md) | All REST endpoints |
| [CLI reference](./reference/cli.md) | The full command-line surface |
| [Deployment](./reference/deploy.md) | systemd, environment variables, the deployment trio |

The Simplified Chinese source of truth lives in `docs/zh-Hans/`; the other languages are translated
from it batch by batch.

Source: [noa](https://github.com/celestia-island/noa).
