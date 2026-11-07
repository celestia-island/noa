//! Line-level three-way merge (W2④): diff3 over the common base, so
//! non-overlapping edits to the same file merge cleanly instead of
//! collapsing into a whole-blob conflict (the feasibility report's hard
//! gap ④ — "真实协作每天撞冲突墙").
//!
//! Algorithm (classic diff3): diff base↔ours and base↔theirs into base-
//! coordinate change hunks, sweep both hunk lists, and group hunks whose
//! base ranges overlap **or touch** into conflict blocks (adjacency
//! conflicts, matching git). A region only one side touched takes that
//! side. Conflict blocks render git-style markers; the caller keeps the
//! structured whole-blob conflict for panel resolution.

/// One base-coordinate change region from one side.
#[derive(Debug, Clone)]
struct Hunk {
    /// Inclusive start line in the BASE (0-based).
    base_lo: usize,
    /// Exclusive end line in the BASE (== base_lo for pure inserts).
    base_hi: usize,
    /// Replacement lines (possibly empty for a delete).
    lines: Vec<String>,
}

/// Result of a line-level three-way merge.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LineMerge {
    /// No overlapping edits — merged text is final.
    Clean(String),
    /// Overlapping edits — `merged` carries git-style conflict markers; the
    /// caller keeps the structured whole-blob conflict for panel resolution.
    Conflict {
        merged: String,
        ours: String,
        theirs: String,
        base: String,
    },
}

/// Changed hunks of `new` relative to `base` in base line coordinates.
/// `new` is split into lines once; hunk `lines` reference those splits (the
/// earlier version sliced the RAW STRING with line indices, producing
/// byte-garbage hunks — the garbled-merge bug).
fn changed_hunks(base: &str, new: &str) -> Vec<Hunk> {
    let new_lines = split_lines(new);
    let diff = similar::TextDiff::from_lines(base, new);
    let mut hunks = Vec::new();
    for op in diff.ops() {
        use similar::DiffOp::*;
        match *op {
            Equal { .. } => {}
            Delete {
                old_index, old_len, ..
            } => hunks.push(Hunk {
                base_lo: old_index,
                base_hi: old_index + old_len,
                lines: Vec::new(),
            }),
            Insert {
                old_index,
                new_index,
                new_len,
                ..
            } => hunks.push(Hunk {
                base_lo: old_index,
                base_hi: old_index,
                lines: new_lines[new_index..new_index + new_len].to_vec(),
            }),
            Replace {
                old_index,
                old_len,
                new_index,
                new_len,
                ..
            } => hunks.push(Hunk {
                base_lo: old_index,
                base_hi: old_index + old_len,
                lines: new_lines[new_index..new_index + new_len].to_vec(),
            }),
        }
    }
    hunks
}

/// Splits text into lines without terminators: "a\nb\n" → ["a", "b"].
fn split_lines(text: &str) -> Vec<String> {
    if text.is_empty() {
        return Vec::new();
    }
    let lines: Vec<String> = text
        .split_inclusive('\n')
        .map(|l| l.trim_end_matches('\n').to_string())
        .collect();
    // A text not ending in \n leaves the last line intact; a text ending in
    // \n produces no trailing empty element with split_inclusive ✓.
    lines
}

/// Joins lines with `\n`, terminating the final line (repo convention LF).
fn join_lines(lines: &[String]) -> String {
    if lines.is_empty() {
        return String::new();
    }
    let mut out = lines.join("\n");
    out.push('\n');
    out
}

/// Classic diff3 sweep: merge two edit sets against a common base.
pub fn merge_lines(base: &str, ours: &str, theirs: &str) -> LineMerge {
    // Identical-sides shortcuts (the common merge shapes).
    if ours == theirs {
        return LineMerge::Clean(ours.to_string());
    }
    if ours == base {
        return LineMerge::Clean(theirs.to_string());
    }
    if theirs == base {
        return LineMerge::Clean(ours.to_string());
    }

    let base_lines = split_lines(base);
    let ours_hunks = changed_hunks(base, ours);
    let theirs_hunks = changed_hunks(base, theirs);

    let mut merged: Vec<String> = Vec::new();
    let mut conflicts = false;
    let mut bi = 0usize; // base cursor
    let mut oi = 0usize;
    let mut ti = 0usize;

    while oi < ours_hunks.len() || ti < theirs_hunks.len() {
        // Pick the side whose hunk starts first.
        let ours_first = match (ours_hunks.get(oi), theirs_hunks.get(ti)) {
            (Some(o), Some(t)) => o.base_lo <= t.base_lo,
            (Some(_), None) => true,
            (None, Some(_)) => false,
            (None, None) => break,
        };

        // Group start: the earlier hunk's base range. Absorb every hunk
        // (either side) whose range touches the growing group (base_lo <=
        // hi — overlap OR adjacency, matching git), transitively.
        let (mut lo, mut hi) = if ours_first {
            let o = &ours_hunks[oi];
            (o.base_lo, o.base_hi.max(o.base_lo))
        } else {
            let t = &theirs_hunks[ti];
            (t.base_lo, t.base_hi.max(t.base_lo))
        };
        let mut group_ours: Vec<&Hunk> = Vec::new();
        let mut group_theirs: Vec<&Hunk> = Vec::new();
        loop {
            let mut grew = false;
            if let Some(o) = ours_hunks.get(oi) {
                if o.base_lo <= hi {
                    group_ours.push(o);
                    lo = lo.min(o.base_lo);
                    hi = hi.max(o.base_hi.max(o.base_lo));
                    oi += 1;
                    grew = true;
                }
            }
            if let Some(t) = theirs_hunks.get(ti) {
                if t.base_lo <= hi {
                    group_theirs.push(t);
                    lo = lo.min(t.base_lo);
                    hi = hi.max(t.base_hi.max(t.base_lo));
                    ti += 1;
                    grew = true;
                }
            }
            if !grew {
                break;
            }
        }
        let group_lo = lo;
        let group_hi = hi;

        // Base lines neither side touched (before the group) — needed by the
        // conflict branch (markers replace the whole region); the single-side
        // branches emit their own per-hunk gaps.
        if !group_theirs.is_empty() && !group_ours.is_empty() && group_lo > bi {
            merged.extend(base_lines[bi..group_lo].iter().cloned());
        }

        if group_theirs.is_empty() {
            // Only ours touched this region — take ours.
            for h in &group_ours {
                if h.base_lo > bi {
                    merged.extend(base_lines[bi..h.base_lo].iter().cloned());
                }
                merged.extend(h.lines.iter().cloned());
                bi = h.base_hi.max(bi);
            }
        } else if group_ours.is_empty() {
            // Only theirs touched this region — take theirs.
            for h in &group_theirs {
                if h.base_lo > bi {
                    merged.extend(base_lines[bi..h.base_lo].iter().cloned());
                }
                merged.extend(h.lines.iter().cloned());
                bi = h.base_hi.max(bi);
            }
        } else {
            // Both sides: conflict block, git-style markers. Each side's
            // region is rendered from ITS OWN hunks + base gaps.
            merged.push("<<<<<<< ours".to_string());
            let mut base_at = group_lo;
            for h in &group_ours {
                if h.base_lo > base_at {
                    merged.extend(base_lines[base_at..h.base_lo].iter().cloned());
                }
                merged.extend(h.lines.iter().cloned());
                base_at = h.base_hi.max(base_at);
            }
            if base_at < group_hi {
                merged.extend(base_lines[base_at..group_hi].iter().cloned());
            }
            merged.push("=======".to_string());
            let mut base_at = group_lo;
            for h in &group_theirs {
                if h.base_lo > base_at {
                    merged.extend(base_lines[base_at..h.base_lo].iter().cloned());
                }
                merged.extend(h.lines.iter().cloned());
                base_at = h.base_hi.max(base_at);
            }
            if base_at < group_hi {
                merged.extend(base_lines[base_at..group_hi].iter().cloned());
            }
            merged.push(">>>>>>> theirs".to_string());
            conflicts = true;
        }
        bi = group_hi.max(bi);
    }

    // Trailing base lines after the last hunk.
    if bi < base_lines.len() {
        merged.extend(base_lines[bi..].iter().cloned());
    }

    let merged_text = join_lines(&merged);
    if conflicts {
        LineMerge::Conflict {
            merged: merged_text,
            ours: ours.to_string(),
            theirs: theirs.to_string(),
            base: base.to_string(),
        }
    } else {
        LineMerge::Clean(merged_text)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_non_overlapping_edits_merge_cleanly() {
        let base = "alpha\nbravo\ncharlie\n";
        let ours = "ALPHA\nbravo\ncharlie\n";
        let theirs = "alpha\nbravo\nCHARLIE\n";
        match merge_lines(base, ours, theirs) {
            LineMerge::Clean(merged) => {
                assert_eq!(merged, "ALPHA\nbravo\nCHARLIE\n");
            }
            LineMerge::Conflict { merged, .. } => {
                panic!("non-overlapping edits must merge cleanly: {merged:?}");
            }
        }
    }

    #[test]
    fn test_same_region_edits_conflict() {
        let base = "alpha\nbravo\n";
        let ours = "ALPHA\nbravo\n";
        let theirs = "alpha-prime\nbravo\n";
        match merge_lines(base, ours, theirs) {
            LineMerge::Clean(_) => panic!("same-region edits must conflict"),
            LineMerge::Conflict {
                merged,
                ours: o,
                theirs: t,
                base: b,
            } => {
                assert!(merged.contains("<<<<<<< ours"));
                assert!(merged.contains("======="));
                assert!(merged.contains(">>>>>>> theirs"));
                assert_eq!(o, ours);
                assert_eq!(t, theirs);
                assert_eq!(b, base);
            }
        }
    }

    #[test]
    fn test_adjacent_edits_conflict_like_git() {
        let base = "one\ntwo\nthree\n";
        let ours = "ONE\ntwo\nthree\n";
        let theirs = "one\nTWO\nthree\n";
        assert!(matches!(
            merge_lines(base, ours, theirs),
            LineMerge::Conflict { .. }
        ));
    }

    #[test]
    fn test_identical_sides_shortcuts() {
        assert!(matches!(
            merge_lines("a\n", "b\n", "b\n"),
            LineMerge::Clean(m) if m == "b\n"
        ));
        assert!(matches!(
            merge_lines("a\n", "a\n", "b\n"),
            LineMerge::Clean(m) if m == "b\n"
        ));
        assert!(matches!(
            merge_lines("a\n", "b\n", "a\n"),
            LineMerge::Clean(m) if m == "b\n"
        ));
    }

    #[test]
    fn test_ours_deletes_theirs_untouched() {
        let base = "keep\ndelete-me\nkeep-too\n";
        let ours = "keep\nkeep-too\n";
        let theirs = "keep\ndelete-me\nkeep-too\n";
        match merge_lines(base, ours, theirs) {
            LineMerge::Clean(m) => assert_eq!(m, "keep\nkeep-too\n"),
            LineMerge::Conflict { .. } => panic!("one-side delete must merge"),
        }
    }

    #[test]
    fn test_insertions_on_different_sides() {
        let base = "middle\n";
        let ours = "top\nmiddle\n";
        let theirs = "middle\nbottom\n";
        match merge_lines(base, ours, theirs) {
            LineMerge::Clean(m) => assert_eq!(m, "top\nmiddle\nbottom\n"),
            LineMerge::Conflict { merged, .. } => {
                panic!("disjoint inserts must merge: {merged:?}")
            }
        }
    }
}
