# Getting Started

## Prerequisites

- Rust 1.75+ (stable)
- Python 3.8+ (for build scripts)
- `just` command runner

## Installation

```bash
git clone https://github.com/celestia-island/noa.git
cd noa
just init          # fetch dependencies
just build-dev     # development build
```

The `noa` binary is at `target/debug/noa`.

## Quick Start

```bash
# Initialize a new repository
noa init .

# Check status
noa status
# On workspace: default

# Create a workspace
noa workspace create feature-1

# Switch to it
noa workspace switch feature-1

# Create a snapshot
noa snapshot create -m "initial work"

# View history
noa log

# Switch back and merge
noa workspace switch default
noa workspace merge feature-1

# Manage remotes
noa remote add origin https://github.com/example/repo.git
noa remote list
```

## Running Examples

```bash
python3 examples/run_all.py
```

## Development

```bash
just fmt            # format code
just clippy         # lint
just test           # run tests
just check          # type-check
```
