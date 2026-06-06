# Git Backend Choice: gix (gitoxide) vs git2 (libgit2)

## Status: Analysis

**Current**: `git2 = "0.19"` (C binding to libgit2)
**Proposed**: `gix = "0.84"` (pure-Rust git implementation)

## Summary

gix (gitoxide) is a mature pure-Rust Git implementation with sufficient
feature coverage to replace git2 for noa's git bridge. The migration
eliminates a C dependency (libgit2), reduces cross-compilation friction,
and provides idiomatic Rust APIs.

## Comparison Matrix

| Criterion | git2 (libgit2) | gix (gitoxide) |
|-----------|---------------|----------------|
| **Language** | C (Rust bindings via git2 crate) | Pure Rust |
| **Maturity** | 14 years, production-proven | 5 years, active development (0.84) |
| **Compilation** | ~15s (rebuild), requires CMake + libgit2-dev | ~8s (rebuild), cargo-only |
| **Cross-compile** | PITA (needs C cross-toolchain) | Trivial (cargo cross-compile) |
| **API style** | C-like, unsafe blocks, manual lifetimes | Rust-idiomatic, borrow-safe, builder patterns |
| **Object handling** | git2::Blob, Tree, Commit via ODB | gix::objs::BlobRef, TreeRef, CommitRef |
| **Tree traversal** | Manual iterator with .to_object() | breadthfirst/virtual_roots with delegate |
| **Remote push/pull** | git2::Remote (fetch, push) | gix::remote (connect, fetch, push) |
| **Pack/pack-index** | Built-in | Comprehensive (own crate: gix-pack) |
| **Refs** | git2::Reference (read/write) | gix::refs (full transaction support) |
| **Config** | Limited (repo-level) | Layered config (system, user, repo) |
| **SHA-1/256** | SHA-1 only | SHA-1 + SHA-256 (experimental) |
| **Memory safety** | Risk from libgit2 C bugs | Rust guarantees |
| **Auditability** | Need to audit libgit2 C codebase | Rust-only, cargo-audit |
| **Community** | Massive (all major VCS tools) | Growing (gitoxide, crates-index-diff, etc.) |

## noa's Git Bridge Requirements

Current usage in `src/git/`:

```rust
// import.rs:
//   - Repository::open()           → gix::open()
//   - repo.head().target()         → gix.head().project_id()
//   - repo.find_commit(oid)        → gix.find_object().try_into_commit()
//   - commit.tree()                → gix.find_object(commit.tree()).try_into_tree()
//   - tree.iter()                  → gix::objs::TreeRefIter
//   - entry.to_object(repo)        → gix.find_object(entry.oid())
//   - obj.kind() === Blob          → obj.kind == ObjectKind::Blob
//   - blob.content()               → blob.data

// translate.rs:
//   - Pure byte-level manipulation (no external git dependency)

// export.rs:
//   - Currently todo!() — push would use gix::remote::connect()
//   - Pack file generation via gix-pack (if needed)
```

All 6 current API calls have direct gix equivalents.

## gix Feature Coverage for noa

| Feature needed | git2 support | gix support | Notes |
|---------------|-------------|-------------|-------|
| Open repo | ✅ | ✅ | `gix::open()` or `gix::ThreadSafeRepository::open()` |
| Read HEAD ref | ✅ | ✅ | `gix.head_ref()` / `gix.head()` |
| Find commit by OID | ✅ | ✅ | `gix.find_object(id)?.try_into_commit()` |
| Read tree from commit | ✅ | ✅ | `gix.find_object(commit.tree())?.try_into_tree()` |
| Iterate tree entries | ✅ | ✅ | `tree.iter()` returns `TreeRefIter` |
| Read blob content | ✅ | ✅ | `blob.data` on `BlobRef` |
| Fetch from remote | ✅ | ✅ | `gix::remote::connect()`  |
| Push to remote | ✅ | ✅ | `gix::remote::connect()`  |
| Clone | ✅ | ✅ | `gix::prepare_clone()` |
| Pack file generation | ✅ | ✅ | `gix-pack` crate |
| SHA-256 support | ❌ | ✅ (experimental) | Relevant for SHA-256 snapshots |
| Async support | ❌ | ✅ (opt-in) | Good for tokio integration |

## Feasibility

All current and planned git operations have gix equivalents. The API mapping
is straightforward:

```rust
// git2 (current)
let repo = git2::Repository::open(path)?;
let head = repo.head()?;
let commit = repo.find_commit(head.target().unwrap())?;
let tree = commit.tree()?;

// gix (proposed)
let repo = gix::open(path)?;
let head = repo.head_ref()?.expect("HEAD not found");
let head_id = head.id().detach();
let commit = repo.find_object(head_id)?.try_into_commit()
    .map_err(|_| NoaError::Remote("not a commit".into()))?;
let tree = repo.find_object(commit.tree())?.try_into_tree()
    .map_err(|_| NoaError::Remote("not a tree".into()))?;
```

## Migration Plan

### Phase 1: Replace import.rs (read-only ops)
- Replace git2::Repository with gix::ThreadSafeRepository
- Reimplement tree traversal
- Run existing git import tests

### Phase 2: Replace translate.rs
- No changes needed (pure byte manipulation, no C dependency)

### Phase 3: Implement export.rs via gix
- Use gix::remote for push
- Use gix::prepare_clone for clone
- Use gix-pack for packfile generation (if needed for server-side)

### Phase 4: Remove git2 from Cargo.toml
- Drop libgit2 system dependency
- Verify cross-compilation (x86_64 → aarch64, → wasm in future)

## Risk Assessment

| Risk | Likelihood | Impact | Mitigation |
|------|-----------|--------|------------|
| gix API breakage (0.x) | Medium | Low | Pin version, adapt to API changes |
| Missing advanced features | Low | Medium | gix has remote push/fetch since 0.50+ |
| Performance regression | Low | Low | gix often faster (no C FFI overhead) |
| Community adoption risk | Low | Low | gix is the de facto Rust git lib |
| SHA-256 interop bugs | Medium | Low | Feature-gated, bypass via pure translate.rs |

## Recommendation

**Migrate to gix.** The benefits (zero C dependencies, pure-Rust safety,
easier cross-compilation, SHA-256 support) outweigh the risks (0.x API
stability, smaller community). The migration is low-risk because:

1. Current git2 usage is minimal (6 API calls in import.rs)
2. translate.rs requires no changes
3. export.rs is unimplemented (greenfield for gix)
4. gix is the standard Rust git library (used by crates.io index)

## Dependencies After Migration

```diff
- git2 = "0.19"           # libgit2 C binding
+ gix = { version = "0.84", features = ["basic", "index", "pack"] }
```

No new system dependencies. Pure `cargo build`.
