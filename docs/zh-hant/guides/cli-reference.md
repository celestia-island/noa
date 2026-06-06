# CLI 參考

## `noa init [path]`

初始化新的 `.noa/` 儲存庫。建立 `noa.redb`、`agent-logs/`、`HEAD` 和 `config`。

```bash
noa init .           # 目前目錄
noa init /path/repo  # 指定路徑
```

## `noa status`

顯示目前工作區和 head 快照。

```bash
noa status
# On workspace: default (head: noa_abc123, msg: initial)
```

## `noa log [options]`

檢視快照歷史記錄。

| 旗標 | 預設值 | 描述 |
|------|---------|-------------|
| `-w, --workspace` | 目前 HEAD | 依工作區篩選 |
| `-l, --limit` | 20 | 顯示的最大條目數 |

```bash
noa log
noa log --workspace feature-1 --limit 50
```

## `noa snapshot <subcommand>`

### `noa snapshot create [-m msg] [-a author]`

從目前工作區的代理日誌建立快照。

```bash
noa snapshot create -m "add login feature" -a "agent-001"
```

### `noa snapshot list`

列出所有工作區中的快照。

### `noa snapshot diff <a> <b>`

顯示兩個快照之間的檔案層級差異。

```bash
noa snapshot diff noa_abc123 noa_def456
```

## `noa workspace <subcommand>`

### `noa workspace create <name> [--agent <id>]`

從目前的 HEAD 分支建立新的工作區。

### `noa workspace switch <name>`

切換使用中的工作區（更新 HEAD）。

### `noa workspace list`

列出所有工作區。`*` 標記使用中的工作區。

### `noa workspace delete <name>`

刪除工作區（無法刪除使用中的工作區）。

### `noa workspace merge <from>`

使用三方合併將另一個工作區合併到目前的工作區。

```bash
noa workspace switch default
noa workspace merge feature-1
```

## `noa remote <subcommand>`

### `noa remote add <name> <url>`

新增遠端儲存庫。

### `noa remote remove <name>`

移除遠端儲存庫。

### `noa remote list`

列出所有已設定的遠端儲存庫。

## `noa push [--remote name]`

推送到遠端儲存庫（尚未實作）。

## `noa pull [--remote name]`

從遠端儲存庫拉取（尚未實作）。

## `noa fetch [--remote name]`

從遠端儲存庫擷取而不合併（尚未實作）。

## `noa clone <url> [path]`

複製遠端儲存庫（尚未實作）。
