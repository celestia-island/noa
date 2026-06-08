# Noa ↔ Entelecheia Integration Plan

> **Last updated**: 2026-06-08
> **Status**: Noa-side Polemos integration implemented. Core handshake, event sync, transport, and server modules are complete.

## Overview

Noa provides git-native workspace versioning for Entelecheia agents. When an agent operates on a workspace, Noa records file writes, creates snapshots at task boundaries, and enables branch-based development (feature branches for agent tasks, rollbacks on failure).

This plan defines the Noa-side responsibilities in the mutual handshake protocol between Noa (source machine) and Scepter (Entelecheia server). The protocol rides on Polemos — Entelecheia's bidirectional JSON-RPC relay over Unix socket / WebSocket.

## Protocol: Noa Handshake over Polemos

### Message Sequence

```
Scepter                                          Noa (via TUI/CLI/WebUI)
   │                                                    │
   │── RequestNoaHandshake {workspace_id, remote} ──────►│
   │                                                    │ 1. Create .noa/ dir
   │                                                    │ 2. Check .gitignore
   │                                                    │ 3. Init libnoa::repo
   │◄── NoaHandshakeResponse {repo_id, branch, ...} ────│
   │                                                    │
   │── NoaAuthRequest {branches[], suggested} ──────────►│
   │                                                    │ 4. Show branch UI
   │◄── NoaAuthResponse {selected_branch} ──────────────│
   │                                                    │
   │── NoaReady {branch, snapshot_id} ──────────────────►│
   │                                                    │
   │◄──────────────── NoaEventSync ─────────────────────►│  Bidirectional
```

### Noa-Side Responsibilities

#### 1. Receive `RequestNoaHandshake` → Initialize `.noa` on source

```
Input:  { workspace_id, remote_name, remote_path }
Output: NoaHandshakeResponse { repo_id, current_branch, noa_initialized, gitignore_updated }

Actions:
  a. Resolve workspace root from remote_path (or cwd if local)
  b. Check if .noa/ directory exists
  c. If not exists:
     - Create .noa/ directory
     - Initialize noa repo (libnoa::repo::Repository::init)
     - Create subdirs: .noa/agent-log/, .noa/snapshots/
  d. Check .gitignore for ".noa" entry
  e. If .noa NOT in .gitignore:
     - Determine current branch via git
     - Create temp branch: "entelecheia/noa-init-{timestamp}"
     - Append ".noa/" to .gitignore
     - Stage .gitignore change
     - Commit: "chore: add .noa to .gitignore"
     - Set gitignore_updated = true
  f. If .noa already in .gitignore:
     - gitignore_updated = false (no changes needed)
  g. Send NoaHandshakeResponse
```

#### 2. Receive `NoaAuthRequest` → Branch Selection UI

```
Input:  { workspace_id, branches[], suggested_branch, reason }
Output: NoaAuthResponse { workspace_id, selected_branch, branch_base, approved }

Actions:
  a. Present auth dialog to user (CLI text menu, TUI page, WebUI modal)
     Options:
       [1] New branch: entelecheia/agent-{session} (suggested)
       [2] New branch: entelecheia/agent-{task_name}
       [3] Use current branch: {current_branch}
       [4] Specify custom branch name: _________
       [x] Cancel (abort noa workspace)
  b. If user selects new branch:
     - git checkout -b {selected_branch} {branch_base}
     - branch_base = current branch at time of selection
  c. If user selects existing branch:
     - git checkout {selected_branch}
     - branch_base = selected_branch
  d. If user cancels:
     - approved = false
     - Revert .gitignore change if made
  e. Send NoaAuthResponse
```

#### 3. Receive `NoaReady` → Acknowledge workspace setup

```
Input:  { workspace_id, branch, snapshot_id }
Actions:
  - Log: Noa workspace {workspace_id} ready on branch {branch}
  - Update local workspace state
  - No response needed (ack already implicit)
```

#### 4. Bidirectional `NoaEventSync`

```
Push (Noa → Scepter):
  - User makes local changes on source machine
  - Noa records events (file edits, git commits)
  - Periodic or on-event push of new events to scepter
  - Scepter applies events to staging copy

Pull (Scepter → Noa):
  - Agent makes changes in #demiurge container
  - NoaToolHook records events
  - Scepter pushes events to source via Polemos
  - Source machine applies events (file writes, branch changes)
  - Note: pull events may require user confirmation (agent modified user's workspace)
```

### Noa Client Library API (to be implemented)

```rust
// In libnoa or a noa-client shim:

/// Initialize .noa on the source machine for a Polemos workspace
pub fn init_noa_for_polemos(
    workspace_root: &Path,
) -> Result<NoaInitResult> {
    // 1. Create .noa/ directory structure
    // 2. Init libnoa::repo::Repository
    // 3. Check and update .gitignore
}

pub struct NoaInitResult {
    pub repo_id: String,
    pub current_branch: String,
    pub noa_initialized: bool,
    pub gitignore_updated: bool,
    pub temp_branch_created: Option<String>,
}

/// Resolve branch selection based on user choice
pub fn select_agent_branch(
    workspace_root: &Path,
    selection: BranchSelection,
    base_branch: Option<&str>,
) -> Result<String> {
    // git checkout -b {branch} {base}
    // or git checkout {existing_branch}
}

pub enum BranchSelection {
    NewSession(String),       // session ID
    NewTask(String),          // task name
    Existing(String),         // branch name
    Current,                  // stay on current branch
}
```

### Configuration

| Env / Config | Default | Description |
|-------------|---------|-------------|
| `NOA_REPO_PATH` | `$PWD/.noa` | Path to noa repository |
| `NOA_AUTO_GITIGNORE` | `false` | Auto-update .gitignore without prompting |
| `NOA_DEFAULT_BRANCH_PREFIX` | `entelecheia/agent-` | Prefix for agent work branches |
| `NOA_SYNC_INTERVAL` | `30s` | Interval for periodic noa event sync |

### Edge Cases

| Scenario | Handling |
|----------|----------|
| `.noa` directory exists but is corrupted | Delete and re-init, warn user |
| Source machine has uncommitted changes | Warn user, suggest commit before agent branch |
| Agent branch already exists | Use existing branch, do not recreate |
| User declines branch auth | Clean up .noa init, abort workspace open |
| Noa sync fails (network) | Queue events locally, retry on reconnect |
| Workspace has no git repo | Error: Noa requires a git repository |
| `.gitignore` is missing | Create `.gitignore` with `.noa/` entry |

### Integration Test Plan

```
Test 1: Noa Init Flow
  - Start scepter
  - TUI connects with NoaWorkspace capability
  - Open noa://localhost/test-project
  - Verify .noa/ created on source
  - Verify .gitignore updated (or not if already present)
  - Verify NoaHandshakeResponse sent

Test 2: Branch Auth Flow
  - Open noa workspace
  - Receive NoaAuthRequest
  - Select "new session branch"
  - Verify git branch created
  - Verify NoaAuthResponse sent

Test 3: Event Sync
  - Agent writes file in container
  - NoaToolHook records event
  - Verify NoaEventSync pushed to source
  - Verify event applied on source machine

Test 4: Snapshot Create/Restore
  - Agent creates snapshot
  - Verify snapshot visible in .noa/snapshots/
  - Checkout snapshot
  - Verify workspace state restored
```
