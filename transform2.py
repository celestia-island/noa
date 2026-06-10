#!/usr/bin/env python3
"""
Second pass: handle remaining NoaError references using multi-line regex.
"""
import re
import os

SRC = "/mnt/sdb1/noa/src"

def read_file(path):
    with open(path, 'r') as f:
        return f.read()

def write_file(path, text):
    with open(path, 'w') as f:
        f.write(text)

def fix_file(relpath, transforms):
    """Apply transforms to file; each transform is (pattern, replacement, flags)."""
    fullpath = os.path.join(SRC, relpath)
    text = read_file(fullpath)
    original = text
    for pattern, replacement, flags in transforms:
        text = re.sub(pattern, replacement, text, flags=flags)
    if text != original:
        write_file(fullpath, text)
        print(f"[OK] {relpath}")
    else:
        print(f"[--] {relpath} (no change)")

# Multi-line patterns
DOTALL = re.DOTALL | re.MULTILINE

# ─── repo.rs ─────────────────────────────────────────────────────
fix_file("repo.rs", [
    # Import
    (r'use crate::\s*\{[^}]*error::\{NoaError,\s*Result\}[^}]*\}',
     lambda m: re.sub(r'error::\{NoaError,\s*Result\},\s*', '', m.group(0)).replace(', }', '}').replace('{\n    \n', '{\n    ').replace('{\n}', ''),
     0),
    (r'use crate::error::\{NoaError, Result\};', 'use crate::error::Result;', 0),
    # RepoAlreadyExists
    (r'return Err\(NoaError::RepoAlreadyExists\(([^)]+)\)\)', r'anyhow::bail!("repository already exists at {}", \1)', 0),
    # RepoNotFound
    (r'return Err\(NoaError::RepoNotFound\(([^)]+)\)\)', r'anyhow::bail!("repository not found at {}", \1)', 0),
    # InvalidRepo
    (r'return Err\(NoaError::InvalidRepo\("([^"]*)"\.to_string\(\)\)\)', r'anyhow::bail!("invalid repository: \1")', 0),
    (r'return Err\(NoaError::InvalidRepo\(([^)]+)\)\)', r'anyhow::bail!("invalid repository: {}", \1)', 0),
    # .map_err(|e| NoaError::Redb(e.to_string())) (no ? at end)
    (r'\.map_err\(\|e\| NoaError::Redb\(e\.to_string\(\)\)\)\s*$', r'.map_err(|e| anyhow::anyhow!("{}", e))?; Ok(())', re.MULTILINE),
    # .map_err(|e| NoaError::Redb(e.to_string()))? 
    (r'\.map_err\(\|e\| NoaError::Redb\(e\.to_string\(\)\)\)\?', '?', 0),
    # txn.commit().map_err(|e| NoaError::Redb(e.to_string()))
    (r'txn\.commit\(\)\.map_err\(\|e\| NoaError::Redb\(e\.to_string\(\)\)\)', 'txn.commit()?;\n        Ok(())', 0),
    # Err(NoaError::Redb(e.to_string()))
    (r'Err\(NoaError::Redb\(([^)]+)\)\)', r'anyhow::bail!("{}", \1)', 0),
    # .map_err(|e| NoaError::Serialization(e.to_string()))?
    (r'\.map_err\(\|e\| NoaError::Serialization\(e\.to_string\(\)\)\)\?', '?', 0),
    # return Err(NoaError::Sync(...)) multi-line
    (r'return Err\(NoaError::Sync\(format!\("([^"]*)",?\s*([^)]*)\)\)\)', r'anyhow::bail!(format!("\1", \2))', 0),
    # get_current_git_branch function
    (r'\.map_err\(\|e\| NoaError::Sync\(format!\("([^"]*)", ([^)]*)\)\)\)\?', r'.with_context(|| format!("\1", \2))?', 0),
    (r'\.map_err\(\|e\| NoaError::Sync\(format!\("([^"]*)"\)\)\)\?', r'.with_context(|| "\1")?', 0),
])

# ─── refs.rs ─────────────────────────────────────────────────────
fix_file("refs.rs", [
    # Remove redb_err from import
    (r',?\s*redb_err', '', 0),
    # Remove NoaError from import (multi-line use crate::{...})
    (r'error::\{NoaError,\s*Result\},?\s*', 'error::Result,\n    ', 0),
    (r'redb_err!\(', '', 0),
    (r'\)\(', ')', 0),  # fix double paren from redb_err!(...) expansion -> ...)
    # Actually redb_err!(expr) expands to expr without the !
    # Let me handle this more carefully
    (r'redb_err!\(([^)]*)\)', r'\1', 0),
    # Fix remaining redb_err
    (r'redb_err!\(', '', 0),
    # .map_err(|e| NoaError::Serialization(e.to_string()))?
    (r'\.map_err\(\|e\| NoaError::Serialization\(e\.to_string\(\)\)\)\?', '?', 0),
    # .map_err(|e| NoaError::Internal(e.to_string()))?
    (r'\.map_err\(\|e\| NoaError::Internal\(e\.to_string\(\)\)\)\?', '?', 0),
    # Err(NoaError::Redb(...))
    (r'Err\(NoaError::Redb\(([^)]+)\)\)', r'anyhow::bail!("{}", \1)', 0),
    # Fix imports cleanup
    (r'use\s+crate::\{\s*\n\s*\}', '', 0),
    (r'use\s+crate::\{\s*\n\s*error::Result,\n\s*\};', 'use crate::error::Result;', 0),
    # ensure_table: redb_err!(txn.commit()) -> txn.commit()?; Ok(())
    (r'redb_err!\(txn\.commit\(\)\)\s*$', 'txn.commit()?;\n        Ok(())', re.MULTILINE),
])

# ─── workspace/mod.rs ────────────────────────────────────────────
fix_file("workspace/mod.rs", [
    # Import: remove NoaError
    (r'error::\{NoaError,\s*Result\},?\s*', 'error::Result,\n    ', 0),
    # Remove redb_err from imports
    (r',?\s*redb_err', '', 0),
    # redb_err!(...) -> ...?
    (r'redb_err!\(', '', 0),
    (r'\)\(', ')', 0),
    (r'redb_err!\(', '', 0),
    # return Err(NoaError::WorkspaceNotFound(...)) multi-line
    (r'return Err\(NoaError::WorkspaceNotFound\("([^"]*)"\.to_string\(\)\)\)', r'anyhow::bail!("\1")', 0),
    (r'return Err\(NoaError::WorkspaceNotFound\(format!\("([^"]*)", ([^)]*)\)\)\)', r'anyhow::bail!(format!("\1", \2))', 0),
    # .ok_or_else(|| NoaError::WorkspaceNotFound(name.clone()))?
    (r'\.ok_or_else\(\|\| NoaError::WorkspaceNotFound\(([^)]+)\)\)\?', r'.ok_or_else(|| anyhow::anyhow!("workspace not found: {}", \1))?', 0),
    # Simple Err(NoaError::WorkspaceAlreadyExists(...))
    (r'Err\(NoaError::WorkspaceAlreadyExists\(([^)]+)\)\)', r'anyhow::bail!("workspace already exists: {}", \1)', 0),
    # .map_err(|e| NoaError::Serialization(...))?
    (r'\.map_err\(\|e\| NoaError::Serialization\(e\.to_string\(\)\)\)\?', '?', 0),
    # .map_err(|e| NoaError::Internal(...))?
    (r'\.map_err\(\|e\| NoaError::Internal\(e\.to_string\(\)\)\)\?', '?', 0),
    # redb_err!(txn.commit()) as last expression -> txn.commit()?; Ok(())
    (r'redb_err!\(txn\.commit\(\)\)', 'txn.commit()?;\n                    Ok(())', 0),
])

# ─── workspace/ops.rs ────────────────────────────────────────────
fix_file("workspace/ops.rs", [
    (r'\.ok_or_else\(\|\| crate::error::NoaError::WorkspaceNotFound\(([^)]+)\)\)\?',
     r'.ok_or_else(|| anyhow::anyhow!("workspace not found: {}", \1))?', 0),
])

# ─── log/file_impl.rs ───────────────────────────────────────────
fix_file("log/file_impl.rs", [
    # Import
    (r'use crate::error::\{NoaError, Result\};', 'use crate::error::Result;', 0),
    # return Err(NoaError::Io(std::io::Error::new(...)))
    (r'return Err\(NoaError::Io\(std::io::Error::new\(([^,]+), ([^)]+)\)\)\)',
     r'anyhow::bail!(std::io::Error::new(\1, \2))', 0),
    # .map_err(|e| NoaError::Internal(e.to_string()))?
    (r'\.map_err\(\|e\| NoaError::Internal\(e\.to_string\(\)\)\)\?', '?', 0),
    # .map_err(NoaError::Io)?
    (r'\.map_err\(NoaError::Io\)\?', '?', 0),
    # Ok::<_, NoaError>( -> Ok(
    (r'Ok::<_, NoaError>\(', 'Ok(', 0),
])

# ─── log/format.rs ───────────────────────────────────────────────
fix_file("log/format.rs", [
    # Import
    (r'use crate::\{[^}]*error::\{NoaError,\s*Result\}[^}]*\}', 'use crate::error::Result;', 0),
    # .map_err(|e| NoaError::Serialization(e.to_string())) (no ?)
    (r'\.map_err\(\|e\| NoaError::Serialization\(e\.to_string\(\)\)\)', '?', 0),
    # Err(NoaError::Serialization("string".to_string()))
    (r'Err\(NoaError::Serialization\("([^"]*)"\.to_string\(\)\)\)', r'anyhow::bail!("\1")', 0),
])

# ─── git/export.rs ───────────────────────────────────────────────
fix_file("git/export.rs", [
    # Import
    (r'use crate::\{[^}]*error::\{NoaError,\s*Result\}[^}]*\}', 
     lambda m: re.sub(r'error::\{NoaError,\s*Result\},\s*', '', m.group(0)).rstrip(',').rstrip() + '\n', 0),
    (r'use crate::error::\{NoaError, Result\};', 'use crate::error::Result;', 0),
    # return Err(NoaError::Remote("string".to_string()))
    (r'return Err\(NoaError::Remote\("([^"]*)"\.to_string\(\)\)\)', r'anyhow::bail!("\1")', 0),
    # return Err(NoaError::Remote(format!(...))) multi-line
    (r'return Err\(NoaError::Remote\(format!\("([^"]*)"\)\)\)', r'anyhow::bail!("\1")', 0),
    (r'''return Err\(NoaError::Remote\(format!\("([^"]*)",\s*([^)]*)\)\)\)''', r'anyhow::bail!(format!("\1", \2))', 0),
    # .map_err(|e| NoaError::Remote(format!(...))) (no ? at end)
    (r'\.map_err\(\|e\| NoaError::Remote\(format!\("([^"]*)", ([^)]*)\)\)\)\s*$', r'.with_context(|| format!("\1", \2))', re.MULTILINE),
    # .map_err(|e| NoaError::Io)?
    (r'\.map_err\(NoaError::Io\)\?', '?', 0),
    # matches!(e, NoaError::WorkspaceAlreadyExists(_))
    (r'matches!\(e, NoaError::WorkspaceAlreadyExists\(_\)\)', 'crate::error::is_workspace_already_exists(&e)', 0),
    # Err(NoaError::Remote(format!(...))) in non-return position
    (r'Err\(NoaError::Remote\(format!\("([^"]*)"\)\)\)', r'anyhow::bail!("\1")', 0),
    # return Err(NoaError::Io(e))
    (r'return Err\(NoaError::Io\(e\)\)', 'return Err(e.into())', 0),
    # .map_err(|e| NoaError::Remote(format!(...)))?
    (r'\.map_err\(\|e\| NoaError::Remote\(format!\("([^"]*)", ([^)]*)\)\)\)\?', r'.with_context(|| format!("\1", \2))?', 0),
    (r'\.map_err\(\|e\| NoaError::Remote\(format!\("([^"]*)"\)\)\)\?', r'.with_context(|| "\1")?', 0),
])

# ─── snapshot/redb_impl.rs ───────────────────────────────────────
fix_file("snapshot/redb_impl.rs", [
    # Import
    (r'error::\{NoaError,\s*Result\},?\s*', 'error::Result,\n    ', 0),
    # redb_err removal
    (r',?\s*redb_err', '', 0),
    (r'redb_err!\(', '', 0),
    (r'redb_err!\(', '', 0),
    # .map_err(|e| NoaError::Serialization(e.to_string())) (no ?)
    (r'\.map_err\(\|e\| NoaError::Serialization\(e\.to_string\(\)\)\)', '?', 0),
    # .map_err(|e| NoaError::Internal(e.to_string()))?
    (r'\.map_err\(\|e\| NoaError::Internal\(e\.to_string\(\)\)\)\?', '?', 0),
    # Err(NoaError::SnapshotNotFound(...))
    (r'Err\(NoaError::SnapshotNotFound\(([^)]+)\)\)', r'anyhow::bail!("snapshot not found: {}", \1)', 0),
    # redb_err!(txn.commit()) as last expr
    (r'redb_err!\(txn\.commit\(\)\)', 'txn.commit()?;\n            Ok(())', 0),
])

# ─── sync/events.rs ──────────────────────────────────────────────
fix_file("sync/events.rs", [
    # Import
    (r'use crate::\{[^}]*error::\{NoaError,\s*Result\}[^}]*\}', 
     lambda m: re.sub(r'error::\{NoaError,\s*Result\},\s*', '', m.group(0)).rstrip(',').rstrip() + '\n', 0),
    # Err(NoaError::ObjectNotFound(_)) in match arm
    (r'Err\(NoaError::ObjectNotFound\(_\)\)', 'Err(e) if crate::error::is_object_not_found(&e)', 0),
    # return Err(NoaError::Io(e))
    (r'return Err\(NoaError::Io\(e\)\)', 'anyhow::bail!(e)', 0),
])

# ─── sync/server.rs ──────────────────────────────────────────────
fix_file("sync/server.rs", [
    # Import
    (r'use crate::error::\{NoaError, Result\};', 'use crate::error::Result;', 0),
    # return Err(NoaError::Sync(format!(...)))
    (r'return Err\(NoaError::Sync\(format!\("([^"]*)",\s*([^)]*)\)\)\)', r'anyhow::bail!(format!("\1", \2))', 0),
    # function signature change: is_eof_error(e: &NoaError) -> is_eof_error(e: &anyhow::Error)
    (r'fn is_eof_error\(e: &NoaError\)', 'fn is_eof_error(e: &anyhow::Error)', 0),
    # matches!(e, NoaError::Io(e) if e.kind() == ... )
    (r'matches!\(e, NoaError::Io\(e\) if e\.kind\(\) == std::io::ErrorKind::UnexpectedEof\)',
     'e.downcast_ref::<std::io::Error>().map_or(false, |io| io.kind() == std::io::ErrorKind::UnexpectedEof)', 0),
])

# ─── sync/handshake.rs ───────────────────────────────────────────
fix_file("sync/handshake.rs", [
    # Import
    (r'use crate::\{[^}]*error::\{NoaError,\s*Result\}[^}]*\}',
     lambda m: re.sub(r'error::\{NoaError,\s*Result\},\s*', '', m.group(0)).rstrip(',').rstrip() + '\n', 0),
    # return Err(NoaError::Sync("string".to_string()))
    (r'return Err\(NoaError::Sync\("([^"]*)"\.to_string\(\)\)\)', r'anyhow::bail!("\1")', 0),
    # return Err(NoaError::Sync(format!(...)))
    (r'return Err\(NoaError::Sync\(format!\("([^"]*)",\s*([^)]*)\)\)\)', r'anyhow::bail!(format!("\1", \2))', 0),
    (r'return Err\(NoaError::Sync\(format!\("([^"]*)"\)\)\)', r'anyhow::bail!("\1")', 0),
    # .map_err(|e| NoaError::Sync(format!(...)))?
    (r'\.map_err\(\|e\| NoaError::Sync\(format!\("([^"]*)",\s*([^)]*)\)\)\)\?', r'.with_context(|| format!("\1", \2))?', 0),
    (r'\.map_err\(\|e\| NoaError::Sync\(format!\("([^"]*)"\)\)\)\?', r'.with_context(|| "\1")?', 0),
])

# ─── config.rs ───────────────────────────────────────────────────
fix_file("config.rs", [
    # .map_err(NoaError::Io) at end of function (no ?)
    (r'\.map_err\(NoaError::Io\)\s*$', '?', re.MULTILINE),
    (r'\.map_err\(NoaError::Io\)\?', '?', 0),
    # .map_err(NoaError::from)
    (r'\.map_err\(NoaError::from\)', '?', 0),
])

# ─── object/redb_impl.rs ─────────────────────────────────────────
fix_file("object/redb_impl.rs", [
    # Import
    (r'error::\{NoaError,\s*Result\},?\s*', 'error::Result,\n    ', 0),
    # redb_err removal
    (r',?\s*redb_err', '', 0),
    (r'redb_err!\(', '', 0),
    # .map_err(|e| NoaError::Serialization(e.to_string())) (no ?)
    (r'\.map_err\(\|e\| NoaError::Serialization\(e\.to_string\(\)\)\)', '?', 0),
    # .map_err(|e| NoaError::Internal(e.to_string()))?
    (r'\.map_err\(\|e\| NoaError::Internal\(e\.to_string\(\)\)\)\?', '?', 0),
    # Err(NoaError::ObjectNotFound(...))
    (r'Err\(NoaError::ObjectNotFound\(([^)]+)\)\)', r'anyhow::bail!("object not found: {}", \1)', 0),
    # redb_err!(txn.commit()) as last expr (in closures)
    (r'redb_err!\(txn\.commit\(\)\)', 'txn.commit()?;\n            Ok(())', 0),
])

# ─── object/minio_impl.rs ────────────────────────────────────────
fix_file("object/minio_impl.rs", [
    # Import
    (r'error::\{NoaError,\s*Result\},?\s*', 'error::Result,\n    ', 0),
    # .map_err(|e| NoaError::Remote(e.to_string()))? -> ?
    (r'\.map_err\(\|e\| NoaError::Remote\(e\.to_string\(\)\)\)\?', '?', 0),
    # .map_err(|e| NoaError::Remote(e.to_string())) (no ?) -> ?
    (r'\.map_err\(\|e\| NoaError::Remote\(e\.to_string\(\)\)\)\s*$', '?', re.MULTILINE),
    # .map_err(|e| NoaError::Serialization(e.to_string())) (no ?) -> ?
    (r'\.map_err\(\|e\| NoaError::Serialization\(e\.to_string\(\)\)\)\s*$', '?', re.MULTILINE),
    # .map_err(|e| NoaError::ObjectNotFound(e.to_string()))? -> ?
    (r'\.map_err\(\|e\| NoaError::ObjectNotFound\(e\.to_string\(\)\)\)\?', '?', 0),
    # Err(NoaError::Config(format!(...)))
    (r'return Err\(NoaError::Config\(format!\("([^"]*)",\s*([^)]*)\)\)\)', r'anyhow::bail!(format!("\1", \2))', 0),
    (r'return Err\(NoaError::Config\(format!\("([^"]*)"\)\)\)', r'anyhow::bail!("\1")', 0),
    (r'(?<!return )Err\(NoaError::Config\(format!\("([^"]*)",\s*([^)]*)\)\)\)', r'anyhow::bail!(format!("\1", \2))', 0),
    (r'(?<!return )Err\(NoaError::Config\(format!\("([^"]*)"\)\)\)', r'anyhow::bail!("\1")', 0),
])

# ─── server/handlers.rs ──────────────────────────────────────────
fix_file("server/handlers.rs", [
    # AppState methods: Result<_, crate::error::NoaError>
    (r'-> Result<RedbObjectStore,\s*crate::error::NoaError>', '-> crate::error::Result<RedbObjectStore>', 0),
    (r'-> Result<RedbSnapshotStore,\s*crate::error::NoaError>', '-> crate::error::Result<RedbSnapshotStore>', 0),
    (r'-> Result<RedbRefStore,\s*crate::error::NoaError>', '-> crate::error::Result<RedbRefStore>', 0),
    (r'-> Result<WorkspaceManager,\s*crate::error::NoaError>', '-> crate::error::Result<WorkspaceManager>', 0),
    # Err(crate::error::NoaError::ObjectNotFound(_))
    (r'Err\(crate::error::NoaError::ObjectNotFound\(_\)\)', 'Err(e) if crate::error::is_object_not_found(&e)', 0),
    # Err(crate::error::NoaError::WorkspaceAlreadyExists(name))
    (r'Err\(crate::error::NoaError::WorkspaceAlreadyExists\((\w+)\)\)',
     r'Err(e) if crate::error::is_workspace_already_exists(&e)', 0),
])

print("\nDone with second pass")
