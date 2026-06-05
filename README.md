# noa — AI-native distributed version control system

<p align="center"><img src="docs/logo.webp" alt="Noa" width="240" /></p>

![Crates.io License](https://img.shields.io/crates/l/noa)
[![Crates.io Version](https://img.shields.io/crates/v/noa)](https://docs.rs/noa)
![GitHub Actions Workflow Status](https://img.shields.io/github/actions/workflow/status/celestia-island/noa/test.yml)

<p align="center"><strong>AI-native distributed version control system</strong></p>

noa is an AI-native distributed version control system. It replaces `.git` with
`.noa/`, using a `redb` embedded KV store for metadata + content-addressed
objects, and per-agent JSONL append-only logs for zero-lock concurrent writes.

**Three design goals:**

1. **Local**: `.noa/` replaces `.git/`. Snapshot-based history. Tens to hundreds
   of AI agents can write simultaneously via isolated incremental logs.
2. **Remote**: 100% compatible with Git protocol (GitHub / Bitbucket / GitLab).
3. **Self-hosted**: `noa-server` provides a native remote with MinIO-backed blob
   storage, merge queue, and agent workspace coordination.

The name `noa` comes from the character [Noa](https://bluearchive.wiki/wiki/Noa)
in the game [Blue Archive](https://bluearchive.jp/).

## Quick Start

```bash
just init          # fetch dependencies
just build-dev     # development build
noa init           # initialize a .noa/ repository
```

## Documentation

- [Building Guide](docs/guides/en/building.md)
- [Architecture](docs/guides/en/architecture.md)
- [Design Documents](docs/design/)

## Related Projects

| Project | Relationship |
|---------|-------------|
| [entelecheia](https://github.com/celestia-island/entelecheia) | Multi-agent orchestration. Consumes noa for version control. |
| [tairitsu](https://github.com/celestia-island/tairitsu) | WASM component model framework. Future: noa client as WASM component. |
| [kirino](https://github.com/celestia-island/kirino) | Zero-trust auth/RBAC. Used by noa-server for authentication. |

## License

Apache-2.0
