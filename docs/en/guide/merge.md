# Merge and Conflicts

noa's merge is a two-layer structure — "tree-level recursion + line-level leaves": directories
recurse level by level, and file-level conflicts first attempt a line-level three-way merge
(diff3), falling back to structured conflicts on failure.

## Line-level three-way merge (W2④)

For both-side modifications to the same file, a diff3 scan runs first with the common ancestor as
base:

- **Regions touched by only one side**: taken from that side directly — non-overlapping edits
  merge automatically and no longer produce whole-blob conflicts;
- **Overlapping or adjacent regions** (consistent with git; zero-gap also counts as a conflict):
  git-style conflict marker blocks are generated (`<<<<<<< ours` / `=======` /
  `>>>>>>> theirs`), and the structured conflict is preserved for panel-side resolution;
- **Fully identical or untouched by one side**: the fast path takes it directly.

```rust
use libnoa::merge::line_merge::{merge_lines, LineMerge};

match merge_lines(base, ours, theirs) {
    LineMerge::Clean(merged) => { /* auto-resolved */ }
    LineMerge::Conflict { merged, ours, theirs, base } => { /* structured conflict */ }
}
```

## Exact content rename (W2④ slice 2)

When one side is a "pure rename" (the old path is deleted and the same blob content reappears
verbatim at the new path) and the other side **modifies the old path**, the modification is
applied automatically at the **new path** — the classic rename+edit scenario no longer produces a
"delete vs modify" conflict.

Undeclared cases (conservatively falling back to structured conflicts):

- the rename also changed the content (payload rename);
- both sides renamed to different targets;
- the target path was touched by the other side (collision);
- rename vs delete (the rename carries the base content and survives at the new path — git
  rename-aware semantics).

Similarity heuristics (rename detection beyond exact matches) are a later slice.

## Degradation semantics

When line-level merge is unavailable (blob missing/corrupt/binary/containing a lone CR), that file
falls back to a structured whole-blob conflict — files are **never** silently dropped, and it
never 500s. The all-empty-on-three-sides fixture shape (storage materializes missing ids as empty
blobs) degrades the same way.

## PR merge flow

`POST /api/v1/repos/{name}/prs/{number}/merge`:

1. three-way snapshot merge (base = PR base, ours = target workspace, theirs = PR head);
2. no conflict → the tree lands automatically + refs advance;
3. conflict → 409 + a conflict list (path + ours/theirs/base blob ids); the panel resolves per
   `ConflictResolution::{Ours, Theirs}`.

## git compatibility

The merged result's blob entries match the git object algebra (blob OIDs
byte-for-byte). The **resolved tree itself is noa-native** (noa SHA-256
addressing): it does not automatically appear in the scratch — pushing it back
to the origin needs snapshot → git-object serialization (the object-level
export is a later slice; the existing working-tree export route achieves the
same thing manually). Until then, merged results live in noa's ref system,
readable as usual through workspaces/snapshots.

