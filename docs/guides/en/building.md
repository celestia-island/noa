# Building noa

## Prerequisites

- Rust 1.75+ (stable)
- Python 3.8+ (for build scripts)
- `just` command runner

## Setup

```bash
git clone https://github.com/celestia-island/noa.git
cd noa
just init          # fetch Rust dependencies
just build-dev     # development build
```

## Development

```bash
just fmt            # format code
just clippy         # lint
just test           # run tests
just check          # type-check
```

## Project Structure

```
src/
├── lib.rs          # Library root
├── error.rs        # Error types
├── config.rs       # Configuration
├── repo.rs         # Repository lifecycle
├── object/         # ObjectStore trait + impls
├── log/            # AgentLog trait + impls
├── snapshot/       # Snapshot engine
├── workspace/      # Workspace manager
├── refs.rs         # RefStore trait + impl
├── merge/          # Merge engine
├── git/            # Git compatibility
├── remote.rs       # RemoteBackend trait
└── cli/            # CLI commands
```
