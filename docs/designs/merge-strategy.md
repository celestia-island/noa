# Merge Strategy Design

## Overview

noa uses a three-way merge algorithm with configurable conflict resolution.
The design prioritizes **forward progress** over human intervention,
reflecting the AI agent use case where changes can be regenerated.

## Three-Way Merge

### Algorithm

Given two snapshots (ours, theirs) with a common ancestor (base):

```
        base
       /    \
    ours    theirs
       \    /
        merge
```

1. Diff `base` vs `ours` → changes_A
2. Diff `base` vs `theirs` → changes_B
3. For each path touched by either:
   - Same change on both sides → apply (no conflict)
   - Changed only in A → apply A
   - Changed only in B → apply B
   - Different changes to same path → **conflict**

### Implementation

```rust
pub fn three_way_merge(
    base_tree: &Vec<TreeEntry>,
    ours_tree: &Vec<TreeEntry>,
    theirs_tree: &Vec<TreeEntry>,
) -> (Vec<TreeEntry>, Vec<Conflict>)
```

Tree entries are normalized into flat path→hash maps for comparison:

```
base:   {src/main.rs: hash1, src/lib.rs: hash2}
ours:   {src/main.rs: hash3, src/lib.rs: hash2}  // modified main.rs
theirs: {src/main.rs: hash1, src/lib.rs: hash4}  // modified lib.rs
Result: {src/main.rs: hash3, src/lib.rs: hash4}   // both applied, no conflict
```

## Conflict Detection

```rust
pub struct Conflict {
    pub path: String,
    pub base_hash: Option<String>,
    pub ours_hash: Option<String>,
    pub theirs_hash: Option<String>,
}
```

Conflict types:
- **Modify/Modify**: Both sides changed the same file differently
- **Add/Add**: Both sides added a file at the same path with different content
- **Delete/Modify**: One side deleted, other modified

## Resolution Strategies

### upstream-wins (default)

When conflict detected, take theirs' version:

```rust
ConflictResolution::UpstreamWins => theirs_hash,
```

Rationale: In AI agent workflows, the "upstream" (main/default workspace)
represents the canonical state. Agents can re-apply their changes against
the updated base.

### ours-wins

Take our version:

```rust
ConflictResolution::OursWins => ours_hash,
```

### fail (planned)

Abort merge and return conflicts for manual resolution:

```rust
ConflictResolution::Fail => return Err(MergeError::Conflict(conflicts)),
```

## Workspace Merge Flow

```bash
noa workspace switch default          # set ours = default
noa workspace merge feature-1         # theirs = feature-1
```

Internal steps:
1. Load ours snapshot (default's head)
2. Load theirs snapshot (feature-1's head)
3. Find merge base (latest common ancestor in DAG)
4. If no common ancestor, use `noa_empty` as base
5. Perform three-way merge
6. Apply conflict resolution strategy
7. Create merge snapshot with parents = [ours, theirs]
8. Update default's head to merge snapshot

## Multi-Parent Merges

noa snapshots support unlimited parents, enabling octopus-style merges:

```
noa_merge (parents: [ws-1, ws-2, ws-3, ..., ws-N])
```

For N-way merges, the algorithm performs pairwise merges:

```
merge(ws-1, ws-2) → intermediate-1
merge(intermediate-1, ws-3) → intermediate-2
...
merge(intermediate-N-2, ws-N) → final
```

## Comparison with Git Merge

| Aspect | noa | Git |
|--------|-----|-----|
| Algorithm | Three-way | Three-way (same core algorithm) |
| Conflict markers | None (auto-resolve) | `<<<<<<<` / `=======` / `>>>>>>>` |
| Default resolution | upstream-wins | None (requires human) |
| Multi-parent | Unlimited | Typically ≤2 |
| Rebase | Not supported | Supported |
| Cherry-pick | Not supported | Supported |
| Fast-forward | Automatic | Optional (–no-ff) |

## Comparison with SVN Merge

| Aspect | noa | SVN |
|--------|-----|-----|
| Merge tracking | Built-in (parent DAG) | Manual (mergeinfo properties) |
| Conflict resolution | Automatic | Manual (conflict files) |
| Branch model | Workspace (lightweight) | Directory-based (heavy) |
| Merge direction | Any → any (DAG) | Typically branch → trunk |

## Design Rationale: Why Auto-Resolve?

Traditional VCS requires human conflict resolution because:
1. Human-written code has semantic meaning that only humans understand
2. Conflicts may represent fundamental design disagreements
3. Manual resolution ensures correctness

AI agent changes have different characteristics:
1. **Regenerable**: Agents can re-apply changes against the latest state
2. **High-frequency**: Pausing for human resolution blocks all downstream work
3. **Non-semantic**: File-level changes don't require human interpretation

Therefore, auto-resolution with a clear policy (upstream-wins) is the
correct trade-off for noa's use case.
