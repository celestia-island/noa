<<<<<<< HEAD
<img src="splash.png" alt="noa" />

![Crates.io License](https://img.shields.io/crates/l/noa)
[![Crates.io Version](https://img.shields.io/crates/v/noa)](https://docs.rs/noa)
![GitHub Actions Workflow Status](https://img.shields.io/github/actions/workflow/status/celestia-island/noa/test.yml)

## Introduction

This is a helper library to generate internationalized markdown documentation via comment macros.

The name `noa` comes from the character [Noa](https://bluearchive.wiki/wiki/Noa) in the game [Blue Archive](https://bluearchive.jp/).

## Quick Start

```rust
use noa::generate_document;

#[generate_document]
pub mod doc {
    /// # Hello
    ///
    /// This is a test.
}
```
=======
<p align="center"><img src="https://raw.githubusercontent.com/celestia-island/docs.celestia.world/dev/res/logo/noa.webp" alt="noa" width="200" /></p>

<h1 align="center">Noa</h1>

<p align="center"><strong>AI-native distributed version control system</strong></p>

<div align="center">

[![License](https://img.shields.io/badge/license-SySL--1.0-blue.svg)](https://sysl.celestia.world)
[![GitHub](https://img.shields.io/badge/github-celestia--island%2Fnoa-blue.svg)](https://github.com/celestia-island/noa)

</div>

<div align="center">

**English** ·
[简体中文](./docs/zhs/intro.md) ·
[繁體中文](./docs/zht/intro.md) ·
[日本語](./docs/ja/intro.md) ·
[한국어](./docs/ko/intro.md) ·
[Français](./docs/fr/intro.md) ·
[Español](./docs/es/intro.md) ·
[Русский](./docs/ru/intro.md) ·
[العربية](./docs/ar/intro.md)

</div>

Per-agent workspace isolation, JSONL append-only logs, snapshot-based history, and full git protocol compatibility.

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
>>>>>>> origin/dev
