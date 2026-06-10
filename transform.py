#!/usr/bin/env python3
"""Transform all NoaError references to anyhow patterns across src/ files."""

import re
import os

SRC = "/mnt/sdb1/noa/src"

# ─── 1. Rewrite error.rs ─────────────────────────────────────────────
def rewrite_error_rs():
    content = """use std::io;

pub type Result<T> = anyhow::Result<T>;

pub fn is_eof_error(e: &anyhow::Error) -> bool {
    e.downcast_ref::<std::io::Error>()
        .map_or(false, |io| io.kind() == std::io::ErrorKind::UnexpectedEof)
        || e.to_string().contains("failed to fill whole buffer")
}

pub fn is_object_not_found(e: &anyhow::Error) -> bool {
    e.to_string().contains("object not found")
}

pub fn is_workspace_already_exists(e: &anyhow::Error) -> bool {
    e.to_string().contains("workspace already exists")
}
"""
    path = os.path.join(SRC, "error.rs")
    with open(path, "w") as f:
        f.write(content)
    print(f"[OK] Rewrote {path}")

# ─── 2. Helper: remove NoaError from imports ────────────────────────
def strip_noaerror_import(text):
    """Transform `use crate::error::{NoaError, Result};` or similar to `use crate::error::Result;`"""
    # Pattern 1: use crate::error::{NoaError, Result};
    text = re.sub(
        r'use\s+crate::error::\{NoaError,\s*Result\};',
        'use crate::error::Result;',
        text,
    )
    # Pattern 2: use crate::{error::{NoaError, Result}, ...}
    text = re.sub(
        r'(use\s+crate::\{)\s*error::\{NoaError,\s*Result\},\s*',
        r'\1',
        text,
    )
    # Clean up empty braces
    text = re.sub(r'use\s+crate::\{\s*};', '', text)
    # Fix `use crate::error::{NoaError, Result},` (in multi-line uses)
    text = re.sub(
        r'use\s+crate::error::\{NoaError,\s*Result\},',
        'use crate::error::Result,',
        text,
    )
    return text

# ─── 3. Generic transformations ─────────────────────────────────────

def transform_redb_err(text):
    """redb_err!(expr)? → expr? and redb_err!(expr) → expr?; Ok(()) for last-expression"""
    # redb_err!(expr)? → expr?
    text = re.sub(r'redb_err!\(([^)]*)\)\?', r'\1?', text)
    return text

def transform_map_err_simple(text):
    """Replace simple map_err patterns"""
    # .map_err(|e| NoaError::Serialization(e.to_string()))? → ?
    text = re.sub(
        r'\.map_err\(\|e\| NoaError::Serialization\(e\.to_string\(\)\)\)\?',
        '?',
        text,
    )
    # .map_err(|e| NoaError::Redb(e.to_string()))? → ?
    text = re.sub(
        r'\.map_err\(\|e\| NoaError::Redb\(e\.to_string\(\)\)\)\?',
        '?',
        text,
    )
    # .map_err(|e| NoaError::Internal(e.to_string()))? → ?
    text = re.sub(
        r'\.map_err\(\|e\| NoaError::Internal\(e\.to_string\(\)\)\)\?',
        '?',
        text,
    )
    # .map_err(NoaError::Io)? → ?
    text = re.sub(r'\.map_err\(NoaError::Io\)\?', '?', text)
    # .map_err(NoaError::from) → ?  (in config.rs)
    text = re.sub(r'\.map_err\(NoaError::from\)', '?', text)
    return text

def transform_ok_type(text):
    """Ok::<_, NoaError>(val) → Ok(val)"""
    text = re.sub(r'Ok::<_, NoaError>\(', 'Ok(', text)
    return text

def transform_return_err_io(text):
    """return Err(NoaError::Io(e)) → Err(e.into())"""
    text = re.sub(
        r'return Err\(NoaError::Io\(e\)\)',
        'Err(e.into())',
        text,
    )
    # Err(NoaError::Io(e)) in non-return position (inside a closure)
    text = re.sub(
        r'(\s*)Err\(NoaError::Io\(e\)\)(?!\s*\()',
        r'\1Err(e.into())',
        text,
    )
    # Err(NoaError::Io(std::io::Error::new(...)))
    text = re.sub(
        r'Err\(NoaError::Io\((std::io::Error::new\([^)]*\))\)\)',
        r'anyhow::bail!(\1)',
        text,
    )
    return text

def transform_remote_map_err(text):
    """Handle .map_err(|e| NoaError::Remote(format!(...)))?
    Convert to .with_context(|| ...)? when format includes {e}
    """
    # Simple case: .map_err(|e| NoaError::Remote(E.to_string()))? → ?
    text = re.sub(
        r'\.map_err\(\|e\| NoaError::Remote\(e\.to_string\(\)\)\)\?',
        '?',
        text,
    )
    # With format containing {e}: .map_err(|e| NoaError::Remote(format!("msg {e}")))?
    # We need anyhow::Context trait import and .with_context(|| format!("msg"))?
    # For now, just use ? and lose the context (the original error type is preserved)
    text = re.sub(
        r'\.map_err\(\|e\| NoaError::Remote\(format!\([^)]*\{e\}[^)]*\)\)\)\?',
        '?',
        text,
    )
    # With simple string context (no {e}) in format
    text = re.sub(
        r'\.map_err\(\|e\| NoaError::Remote\(("[^"]*".*?)\)\)\?',
        r'.with_context(|| \1)?',
        text,
    )
    return text

def transform_sync_map_err(text):
    """Handle .map_err(|e| NoaError::Sync(format!(...)))?"""
    # Simple: .map_err(|e| NoaError::Sync(e.to_string()))? → ?
    text = re.sub(
        r'\.map_err\(\|e\| NoaError::Sync\(e\.to_string\(\)\)\)\?',
        '?',
        text,
    )
    # With format: .map_err(|e| NoaError::Sync(format!("... {e}")))?
    text = re.sub(
        r'\.map_err\(\|e\| NoaError::Sync\(format!\([^)]*\{e\}[^)]*\)\)\)\?',
        '?',
        text,
    )
    # .map_err(|e| NoaError::Sync(format!("... {status}")))?
    text = re.sub(
        r'\.map_err\(\|e\| NoaError::Sync\(format!\(([^)]*)\)\)\)\?',
        r'.with_context(|| format!(\1))?',
        text,
    )
    return text

def transform_config_map_err(text):
    """Handle .map_err(|e| NoaError::Config(format!(...)))?"""
    text = re.sub(
        r'\.map_err\(\|e\| NoaError::Config\(format!\(([^)]*)\)\)\)\?',
        r'.with_context(|| format!(\1))?',
        text,
    )
    return text

def transform_object_not_found(text):
    """Handle Err(NoaError::ObjectNotFound(...))"""
    text = re.sub(
        r'Err\(NoaError::ObjectNotFound\(([^)]+)\)\)',
        r'anyhow::bail!("object not found: {}", \1)',
        text,
    )
    return text

def transform_snapshot_not_found(text):
    """Handle Err(NoaError::SnapshotNotFound(...))"""
    text = re.sub(
        r'Err\(NoaError::SnapshotNotFound\(([^)]+)\)\)',
        r'anyhow::bail!("snapshot not found: {}", \1)',
        text,
    )
    return text

def transform_workspace_not_found(text):
    """Handle Err(NoaError::WorkspaceNotFound(...)) / return Err(NoaError::WorkspaceNotFound(...))"""
    text = re.sub(
        r'Err\(NoaError::WorkspaceNotFound\(([^)]+)\)\)',
        r'anyhow::bail!("workspace not found: {}", \1)',
        text,
    )
    return text

def transform_workspace_already_exists(text):
    """Handle Err(NoaError::WorkspaceAlreadyExists(...))"""
    text = re.sub(
        r'Err\(NoaError::WorkspaceAlreadyExists\(([^)]+)\)\)',
        r'anyhow::bail!("workspace already exists: {}", \1)',
        text,
    )
    return text

def transform_return_statement(text):
    """Handle multi-line return Err(NoaError::Xxx(format!(...))) patterns"""
    # Match return Err(NoaError::Xxx("string".to_string()))
    text = re.sub(
        r'return Err\(NoaError::\w+\(([^)]+\.to_string\(\))\)\);',
        r'anyhow::bail!(\1)',
        text,
    )
    # Match simple Err(NoaError::Xxx("string".to_string())) not preceded by return
    text = re.sub(
        r'(?<!return )Err\(NoaError::(\w+)\(("[^"]*"\.to_string\(\))\)\)',
        r'anyhow::bail!(\2)',
        text,
    )
    return text

def transform_matches_pattern(text):
    """matches!(e, NoaError::WorkspaceAlreadyExists(_)) → crate::error::is_workspace_already_exists(&e)"""
    text = re.sub(
        r'matches!\(e, NoaError::WorkspaceAlreadyExists\(_\)\)',
        'crate::error::is_workspace_already_exists(&e)',
        text,
    )
    return text

def transform_err_match_pattern(text):
    """Err(crate::error::NoaError::ObjectNotFound(_)) → Err(e) if crate::error::is_object_not_found(&e)"""
    text = re.sub(
        r'Err\(crate::error::NoaError::ObjectNotFound\(_\)\)',
        'Err(e) if crate::error::is_object_not_found(&e)',
        text,
    )
    # Also handle Err(NoaError::ObjectNotFound(_)) (without crate::error:: prefix)
    text = re.sub(
        r'Err\(NoaError::ObjectNotFound\(_\)\)',
        'Err(e) if crate::error::is_object_not_found(&e)',
        text,
    )
    return text

def transform_workspace_already_exists_match(text):
    """Err(crate::error::NoaError::WorkspaceAlreadyExists(name)) → ..."""
    text = re.sub(
        r'Err\(crate::error::NoaError::WorkspaceAlreadyExists\((\w+)\)\)',
        r'Err(e) if crate::error::is_workspace_already_exists(&e)',
        text,
    )
    return text

def transform_ok_result(text):
    """Ok::<_, NoaError>(seq) → Ok(seq)"""
    text = re.sub(r'Ok::<_,\s*NoaError>\s*\(', 'Ok(', text)
    return text

def add_anyhow_context_import(text):
    """Add use anyhow::Context; if .with_context(|| appears"""
    if '.with_context(||' in text and 'use anyhow::Context;' not in text:
        # Add after the last use statement or at top
        text = re.sub(
            r'^(use .+)$',
            r'\1\nuse anyhow::Context;',
            text,
            count=1,
            flags=re.MULTILINE,
        )
        # Actually, let's be smarter: add after the last `use crate::` line or similar
    return text

def transform_redb_err_last_expr(text):
    """Handle redb_err!(txn.commit()) at end of function (without ?).
    This needs special handling since it's the last expression.
    After redb_err macro removal, txn.commit() returns Result<(), redb::Error>
    which auto-converts to anyhow::Error via ?.
    """
    # Specific pattern for ensure_tables functions:
    # redb_err!(txn.commit()) → txn.commit()?; Ok(())
    text = re.sub(
        r'redb_err!\(txn\.commit\(\)\)\s*$',
        'txn.commit()?;\n        Ok(())',
        text,
        flags=re.MULTILINE,
    )
    return text

def transform_misc(text):
    """Other miscellaneous patterns"""
    # the `return Err(NoaError::Io(std::io::Error::new(...)))` in log/file_impl.rs
    text = re.sub(
        r'return Err\(NoaError::Io\(std::io::Error::new\(([^)]+), ([^)]+)\)\)\)',
        r'anyhow::bail!(std::io::Error::new(\1, \2))',
        text,
    )
    # .map_err(|e| NoaError::Io(e))? → ? (used in some closures)
    text = re.sub(
        r'\.map_err\(\|e\| NoaError::Io\(e\)\)\?',
        '?',
        text,
    )
    # Ok::<_, NoaError>(seq) → Ok(seq) but with more flexible whitespace
    text = re.sub(
        r'Ok::<_,\s*NoaError>\s*\(',
        'Ok(',
        text,
    )
    # Err(NoaError::Serialization("string".to_string()))
    text = re.sub(
        r'Err\(NoaError::Serialization\("([^"]*)"\.to_string\(\)\)\)',
        r'anyhow::bail!("\1")',
        text,
    )
    # .map_err(|e| NoaError::Serialization("string".to_string()))? 
    # This is unusual but might exist
    text = re.sub(
        r'\.map_err\(\|e\| NoaError::Serialization\("([^"]*)"\.to_string\(\)\)\)\?',
        r'.map_err(|_| anyhow::anyhow!("\1"))?',
        text,
    )
    # return Err(NoaError::Sync("string".to_string()))
    text = re.sub(
        r'return Err\(NoaError::Sync\("([^"]*)"\.to_string\(\)\)\)',
        r'anyhow::bail!("\1")',
        text,
    )
    # Err(NoaError::Sync("string".to_string())) in non-return position
    text = re.sub(
        r'(?<!return )Err\(NoaError::Sync\("([^"]*)"\.to_string\(\)\)\)',
        r'anyhow::bail!("\1")',
        text,
    )
    # return Err(NoaError::Sync(format!(...)))
    text = re.sub(
        r'return Err\(NoaError::Sync\(format!\(([^)]*)\)\)\)',
        r'anyhow::bail!(format!(\1))',
        text,
    )
    # Err(NoaError::Sync(format!(...))) in non-return
    text = re.sub(
        r'(?<!return )Err\(NoaError::Sync\(format!\(([^)]*)\)\)\)',
        r'anyhow::bail!(format!(\1))',
        text,
    )
    # .map_err(|e| NoaError::Remote(format!("... {e}")))?  with more complex pattern
    text = re.sub(
        r'\.map_err\(\|e\| NoaError::Remote\(format!\([^)]*\{e\}[^)]*\)\)\)\?',
        '?',
        text,
    )
    # getrandom errors
    text = re.sub(
        r'NoaError::Internal\(format!\("failed to generate random sync token: \{e\}"\)\)',
        'anyhow::anyhow!("failed to generate random sync token: {e}")',
        text,
    )
    return text

# ─── 4. File-specific handlers ──────────────────────────────────────

def process_config_rs(text):
    text = strip_noaerror_import(text)
    text = transform_map_err_simple(text)  # handles .map_err(NoaError::from) and NoaError::Io
    return text

def process_repo_rs(text):
    text = strip_noaerror_import(text)
    # Err(NoaError::RepoAlreadyExists(...))
    text = re.sub(
        r'Err\(NoaError::RepoAlreadyExists\(([^)]+)\)\)',
        r'anyhow::bail!("repository already exists at {}", \1)',
        text,
    )
    # Err(NoaError::RepoNotFound(...)) for noa_dir.display()
    text = re.sub(
        r'Err\(NoaError::RepoNotFound\(([^)]+)\)\)',
        r'anyhow::bail!("repository not found at {}", \1)',
        text,
    )
    # Err(NoaError::InvalidRepo(...))
    text = re.sub(
        r'Err\(NoaError::InvalidRepo\(([^)]+)\)\)',
        r'anyhow::bail!("invalid repository: {}", \1)',
        text,
    )
    # .map_err(|e| NoaError::Redb(e.to_string()))?
    text = re.sub(
        r'\.map_err\(\|e\| NoaError::Redb\(e\.to_string\(\)\)\)\?',
        '?',
        text,
    )
    # .map_err(|e| NoaError::Redb(format!(...)))?
    text = re.sub(
        r'\.map_err\(\|e\| NoaError::Redb\(format!\(([^)]*)\)\)\)\?',
        r'.map_err(|e| anyhow::anyhow!(format!(\1)))?',
        text,
    )
    # .map_err(|e| NoaError::Serialization(e.to_string()))?
    text = re.sub(
        r'\.map_err\(\|e\| NoaError::Serialization\(e\.to_string\(\)\)\)\?',
        '?',
        text,
    )
    # .map_err(|e| NoaError::Sync(format!(...)))?
    text = re.sub(
        r'\.map_err\(\|e\| NoaError::Sync\(format!\(([^)]*)\)\)\)\?',
        r'.with_context(|| format!(\1))?',
        text,
    )
    # return Err(NoaError::Sync(...))
    text = re.sub(
        r'return Err\(NoaError::Sync\("([^"]*)"\.to_string\(\)\)\)',
        r'anyhow::bail!("\1")',
        text,
    )
    # Err(NoaError::Redb(e.to_string())) in non-map_err position
    text = re.sub(
        r'(?<!map_err\()Err\(NoaError::Redb\(([^)]+)\)\)',
        r'anyhow::bail!("database error: {}", \1)',
        text,
    )
    return text

def process_refs_rs(text):
    text = strip_noaerror_import(text)
    text = transform_redb_err(text)
    text = transform_map_err_simple(text)
    text = transform_ok_type(text)
    # Handle NoaError::Redb in commit error
    text = re.sub(
        r'Err\(NoaError::Redb\(e\.to_string\(\)\)\)',
        'Err(anyhow::anyhow!("{}", e))',
        text,
    )
    # .map_err(|e| NoaError::Serialization(e.to_string()))? in refs.rs
    text = re.sub(
        r'\.map_err\(\|e\| NoaError::Serialization\(e\.to_string\(\)\)\)\?',
        '?',
        text,
    )
    # remove use of redb_err in imports
    text = re.sub(r'use\s+crate::\{[^}]*redb_err[^}]*\}', '', text)
    # Fix empty braces import
    text = re.sub(r'use\s+crate::\{\s*};', '', text)
    # Clean up `use crate::{\n    error::Result,\n};` → `use crate::error::Result;`
    text = re.sub(
        r'use\s+crate::\{\s*\n\s*error::Result,\s*\n\s*\};',
        'use crate::error::Result;',
        text,
    )
    return text

def process_workspace_mod_rs(text):
    text = strip_noaerror_import(text)
    text = transform_redb_err(text)
    text = transform_map_err_simple(text)
    text = transform_workspace_not_found(text)
    text = transform_workspace_already_exists(text)
    # Handle .ok_or_else(|| NoaError::WorkspaceNotFound(...))?
    text = re.sub(
        r'\.ok_or_else\(\|\| NoaError::WorkspaceNotFound\(([^)]+)\)\)\?',
        r'.ok_or_else(|| anyhow::anyhow!("workspace not found: {}", \1))?',
        text,
    )
    # remove `redb_err` from imports
    text = re.sub(r',?\s*redb_err', '', text)
    return text

def process_workspace_ops_rs(text):
    # .ok_or_else(|| crate::error::NoaError::WorkspaceNotFound(...))?
    text = re.sub(
        r'\.ok_or_else\(\|\| crate::error::NoaError::WorkspaceNotFound\(([^)]+)\)\)\?',
        r'.ok_or_else(|| anyhow::anyhow!("workspace not found: {}", \1))?',
        text,
    )
    return text

def process_object_redb_impl_rs(text):
    text = strip_noaerror_import(text)
    text = transform_redb_err(text)
    text = transform_map_err_simple(text)
    text = transform_object_not_found(text)
    # remove `redb_err` from imports
    text = re.sub(r',?\s*redb_err', '', text)
    return text

def process_object_minio_impl_rs(text):
    text = strip_noaerror_import(text)
    text = transform_map_err_simple(text)
    text = transform_object_not_found(text)
    # Err(NoaError::Config(format!(...)))
    text = re.sub(
        r'Err\(NoaError::Config\(format!\(([^)]*)\)\)\)',
        r'anyhow::bail!(format!(\1))',
        text,
    )
    # .map_err(|e| NoaError::Remote(e.to_string()))? → ?
    text = re.sub(
        r'\.map_err\(\|e\| NoaError::Remote\(e\.to_string\(\)\)\)\?',
        '?',
        text,
    )
    # .map_err(|e| NoaError::Remote(e.to_string())) → ...  (last expression, no ?)
    text = re.sub(
        r'\.map_err\(\|e\| NoaError::Remote\(e\.to_string\(\)\)\)\s*$',
        '.map_err(|e| anyhow::anyhow!("{}", e))',
        text,
    )
    # .map_err(|e| NoaError::Serialization(e.to_string())) in last expression (no ?)
    text = re.sub(
        r'\.map_err\(\|e\| NoaError::Serialization\(e\.to_string\(\)\)\)\s*$',
        '?',
        text,
    )
    # .map_err(|e| NoaError::ObjectNotFound(e.to_string()))? → .with_context...
    text = re.sub(
        r'\.map_err\(\|e\| NoaError::ObjectNotFound\(e\.to_string\(\)\)\)\?',
        '?',
        text,
    )
    return text

def process_snapshot_redb_impl_rs(text):
    text = strip_noaerror_import(text)
    text = transform_redb_err(text)
    text = transform_map_err_simple(text)
    text = transform_snapshot_not_found(text)
    text = transform_object_not_found(text)  # also handles ObjectNotFound in children_of
    # remove `redb_err` from imports
    text = re.sub(r',?\s*redb_err', '', text)
    return text

def process_log_file_impl_rs(text):
    text = strip_noaerror_import(text)
    text = transform_map_err_simple(text)  # handles .map_err(NoaError::Io)?
    text = transform_ok_type(text)
    # return Err(NoaError::Io(std::io::Error::new(...)))
    text = re.sub(
        r'return Err\(NoaError::Io\(std::io::Error::new\(([^,]+), ([^)]+)\)\)\)',
        r'anyhow::bail!(std::io::Error::new(\1, \2))',
        text,
    )
    # return Err(NoaError::Io(e))
    text = re.sub(
        r'return Err\(NoaError::Io\(e\)\)',
        'return Err(e.into())',
        text,
    )
    # .map_err(|e| NoaError::Internal(e.to_string()))?
    text = re.sub(
        r'\.map_err\(\|e\| NoaError::Internal\(e\.to_string\(\)\)\)\?',
        '?',
        text,
    )
    # Ok::<_, NoaError>(seq) → Ok(seq)
    text = re.sub(
        r'Ok::<_, NoaError>\(',
        'Ok(',
        text,
    )
    return text

def process_log_format_rs(text):
    text = strip_noaerror_import(text)
    text = transform_map_err_simple(text)
    # Err(NoaError::Serialization("string".to_string()))
    text = re.sub(
        r'Err\(NoaError::Serialization\("([^"]*)"\.to_string\(\)\)\)',
        r'anyhow::bail!("\1")',
        text,
    )
    return text

def process_sync_server_rs(text):
    text = strip_noaerror_import(text)
    text = transform_map_err_simple(text)
    # function signature change: is_eof_error(e: &NoaError) → is_eof_error(e: &anyhow::Error)
    text = re.sub(
        r'fn is_eof_error\(e: &NoaError\)',
        'fn is_eof_error(e: &anyhow::Error)',
        text,
    )
    # is_eof_error(&e) should still work since we changed the function signature
    # matches!(e, NoaError::Io(e) if e.kind() == ... ) → use downcast_ref
    text = re.sub(
        r'matches!\(e, NoaError::Io\(e\) if e\.kind\(\) == std::io::ErrorKind::UnexpectedEof\)',
        'e.downcast_ref::<std::io::Error>().map_or(false, |io| io.kind() == std::io::ErrorKind::UnexpectedEof)',
        text,
    )
    # getrandom error
    text = re.sub(
        r'\.map_err\(\|e\| NoaError::Internal\(format!\("failed to generate random sync token: \{e\}"\)\)\)\?',
        '.map_err(|e| anyhow::anyhow!("failed to generate random sync token: {e}"))?',
        text,
    )
    # .map_err(|e| NoaError::Sync(format!("... {e}")))?
    text = re.sub(
        r'\.map_err\(\|e\| NoaError::Sync\(format!\(([^)]*\{e\}[^)]*)\)\)\)\?',
        r'.with_context(|| format!(\1))?',
        text,
    )
    # .map_err(|e| NoaError::Sync(format!("... {len}")))?
    text = re.sub(
        r'\.map_err\(\|e\| NoaError::Sync\(format!\(([^)]*)\)\)\)\?',
        r'.with_context(|| format!(\1))?',
        text,
    )
    # .map_err(|e| NoaError::Sync("string".to_string()))?
    text = re.sub(
        r'\.map_err\(\|e\| NoaError::Sync\("([^"]*)"\.to_string\(\)\)\)\?',
        r'.with_context(|| "\1")?',
        text,
    )
    # return Err(NoaError::Sync("string".to_string()))
    text = re.sub(
        r'return Err\(NoaError::Sync\("([^"]*)"\.to_string\(\)\)\)',
        r'anyhow::bail!("\1")',
        text,
    )
    # return Err(NoaError::Sync(format!(...)))
    text = re.sub(
        r'return Err\(NoaError::Sync\(format!\(([^)]*)\)\)\)',
        r'anyhow::bail!(format!(\1))',
        text,
    )
    # .map_err(|e| NoaError::Serialization(e.to_string()))?  (in server.rs)
    text = re.sub(
        r'\.map_err\(\|e\| NoaError::Serialization\(e\.to_string\(\)\)\)\?',
        '?',
        text,
    )
    # last expression: .map_err(|e| NoaError::Sync(format!("flush: {e}")))
    text = re.sub(
        r'\.map_err\(\|e\| NoaError::Sync\(format!\(([^)]*)\)\)\)\s*$',
        r'.with_context(|| format!(\1))',
        text,
        flags=re.MULTILINE,
    )
    return text

def process_sync_transport_rs(text):
    text = strip_noaerror_import(text)
    text = transform_map_err_simple(text)
    # Err(NoaError::Sync("string".to_string()))
    text = re.sub(
        r'Err\(NoaError::Sync\("([^"]*)"\.to_string\(\)\)\)',
        r'anyhow::bail!("\1")',
        text,
    )
    # .map_err(|e| NoaError::Serialization(e.to_string()))?
    text = re.sub(
        r'\.map_err\(\|e\| NoaError::Serialization\(e\.to_string\(\)\)\)\?',
        '?',
        text,
    )
    # std::str::from_utf8 error in transport
    text = re.sub(
        r'\.map_err\(\|e\| NoaError::Serialization\(e\.to_string\(\)\)\)',
        '?',
        text,
    )
    return text

def process_sync_events_rs(text):
    text = strip_noaerror_import(text)
    text = transform_object_not_found(text)
    # Err(NoaError::ObjectNotFound(_)) in match arm
    text = re.sub(
        r'Err\(NoaError::ObjectNotFound\(_\)\)',
        'Err(e) if crate::error::is_object_not_found(&e)',
        text,
    )
    return text

def process_sync_handshake_rs(text):
    text = strip_noaerror_import(text)
    # return Err(NoaError::Sync("string".to_string()))
    text = re.sub(
        r'return Err\(NoaError::Sync\("([^"]*)"\.to_string\(\)\)\)',
        r'anyhow::bail!("\1")',
        text,
    )
    # return Err(NoaError::Sync(format!(...)))
    text = re.sub(
        r'return Err\(NoaError::Sync\(format!\(([^)]*)\)\)\)',
        r'anyhow::bail!(format!(\1))',
        text,
    )
    # .map_err(|e| NoaError::Sync(format!(...)))?
    text = re.sub(
        r'\.map_err\(\|e\| NoaError::Sync\(format!\(([^)]*)\)\)\)\?',
        r'.with_context(|| format!(\1))?',
        text,
    )
    return text

def process_git_mod_rs(text):
    text = strip_noaerror_import(text)
    # .map_err(|e| NoaError::Remote(format!("... {e}")))?
    text = re.sub(
        r'\.map_err\(\|e\| NoaError::Remote\(format!\(([^)]*\{e\}[^)]*)\)\)\)\?',
        r'.with_context(|| format!(\1))?',
        text,
    )
    # Err(NoaError::Remote(format!(...)))
    text = re.sub(
        r'Err\(NoaError::Remote\(format!\(([^)]*)\)\)\)',
        r'anyhow::bail!(format!(\1))',
        text,
    )
    return text

def process_git_export_rs(text):
    text = strip_noaerror_import(text)
    # Err(NoaError::Remote("string".to_string()))
    text = re.sub(
        r'return Err\(NoaError::Remote\("([^"]*)"\.to_string\(\)\)\)',
        r'anyhow::bail!("\1")',
        text,
    )
    text = re.sub(
        r'(?<!return )Err\(NoaError::Remote\("([^"]*)"\.to_string\(\)\)\)',
        r'anyhow::bail!("\1")',
        text,
    )
    # return Err(NoaError::Remote(format!(...)))
    text = re.sub(
        r'return Err\(NoaError::Remote\(format!\(([^)]*)\)\)\)',
        r'anyhow::bail!(format!(\1))',
        text,
    )
    # .map_err(|e| NoaError::Remote(format!(...)))?
    text = re.sub(
        r'\.map_err\(\|e\| NoaError::Remote\(format!\(([^)]*)\)\)\)\?',
        r'.with_context(|| format!(\1))?',
        text,
    )
    # .map_err(NoaError::Io)?
    text = re.sub(
        r'\.map_err\(NoaError::Io\)\?',
        '?',
        text,
    )
    # matches!(e, NoaError::WorkspaceAlreadyExists(_))
    text = re.sub(
        r'matches!\(e, NoaError::WorkspaceAlreadyExists\(_\)\)',
        'crate::error::is_workspace_already_exists(&e)',
        text,
    )
    # return Err(NoaError::Io(e))
    text = re.sub(
        r'return Err\(NoaError::Io\(e\)\)',
        'return Err(e.into())',
        text,
    )
    return text

def process_git_import_rs(text):
    text = strip_noaerror_import(text)
    # .map_err(|e| NoaError::Remote(e.to_string()))?
    text = re.sub(
        r'\.map_err\(\|e\| NoaError::Remote\(e\.to_string\(\)\)\)\?',
        '?',
        text,
    )
    # .map_err(|e| NoaError::Remote(format!(...)))?
    text = re.sub(
        r'\.map_err\(\|e\| NoaError::Remote\(format!\(([^)]*)\)\)\)\?',
        r'.with_context(|| format!(\1))?',
        text,
    )
    return text

def process_server_handlers_rs(text):
    # AppState methods that return Result<..., NoaError>
    text = re.sub(
        r'-> Result<[^,]+,\s*crate::error::NoaError>',
        '-> crate::error::Result<_>',
        text,
    )
    # Adjust return types for store methods
    text = re.sub(
        r'Result<RedbObjectStore,\s*crate::error::NoaError>',
        'crate::error::Result<RedbObjectStore>',
        text,
    )
    text = re.sub(
        r'Result<RedbSnapshotStore,\s*crate::error::NoaError>',
        'crate::error::Result<RedbSnapshotStore>',
        text,
    )
    text = re.sub(
        r'Result<RedbRefStore,\s*crate::error::NoaError>',
        'crate::error::Result<RedbRefStore>',
        text,
    )
    text = re.sub(
        r'Result<WorkspaceManager,\s*crate::error::NoaError>',
        'crate::error::Result<WorkspaceManager>',
        text,
    )
    # match arm: Err(crate::error::NoaError::ObjectNotFound(_))
    text = re.sub(
        r'Err\(crate::error::NoaError::ObjectNotFound\(_\)\)',
        'Err(e) if crate::error::is_object_not_found(&e)',
        text,
    )
    # Err(crate::error::NoaError::WorkspaceAlreadyExists(name))
    text = re.sub(
        r'Err\(crate::error::NoaError::WorkspaceAlreadyExists\((\w+)\)\)',
        r'Err(e) if crate::error::is_workspace_already_exists(&e)',
        text,
    )
    return text

# ─── 5. Main ────────────────────────────────────────────────────────

def main():
    rewrite_error_rs()

    # Process all files
    processors = {
        "config.rs": process_config_rs,
        "repo.rs": process_repo_rs,
        "refs.rs": process_refs_rs,
        "workspace/mod.rs": process_workspace_mod_rs,
        "workspace/ops.rs": process_workspace_ops_rs,
        "object/redb_impl.rs": process_object_redb_impl_rs,
        "object/minio_impl.rs": process_object_minio_impl_rs,
        "snapshot/redb_impl.rs": process_snapshot_redb_impl_rs,
        "log/file_impl.rs": process_log_file_impl_rs,
        "log/format.rs": process_log_format_rs,
        "sync/server.rs": process_sync_server_rs,
        "sync/transport.rs": process_sync_transport_rs,
        "sync/events.rs": process_sync_events_rs,
        "sync/handshake.rs": process_sync_handshake_rs,
        "git/mod.rs": process_git_mod_rs,
        "git/export.rs": process_git_export_rs,
        "git/import.rs": process_git_import_rs,
        "server/handlers.rs": process_server_handlers_rs,
    }

    for relpath, processor in processors.items():
        fullpath = os.path.join(SRC, relpath)
        if not os.path.exists(fullpath):
            print(f"[SKIP] {relpath} does not exist")
            continue
        with open(fullpath, "r") as f:
            text = f.read()

        original = text
        text = processor(text)

        if text != original:
            with open(fullpath, "w") as f:
                f.write(text)
            print(f"[OK] Transformed {relpath}")
        else:
            print(f"[UNCHANGED] {relpath}")

if __name__ == "__main__":
    main()
