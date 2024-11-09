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
