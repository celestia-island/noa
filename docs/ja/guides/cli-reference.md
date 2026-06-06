# CLIリファレンス

## `noa init [path]`

新しい`.noa/`リポジトリを初期化します。`noa.redb`、`agent-logs/`、`HEAD`、`config`を作成します。

```bash
noa init .           # カレントディレクトリ
noa init /path/repo  # 特定のパス
```

## `noa status`

現在のワークスペースとヘッドスナップショットを表示します。

```bash
noa status
# On workspace: default (head: noa_abc123, msg: initial)
```

## `noa log [options]`

スナップショット履歴を表示します。

| フラグ | デフォルト | 説明 |
|------|---------|-------------|
| `-w, --workspace` | 現在のHEAD | ワークスペースでフィルタ |
| `-l, --limit` | 20 | 表示する最大エントリ数 |

```bash
noa log
noa log --workspace feature-1 --limit 50
```

## `noa snapshot <subcommand>`

### `noa snapshot create [-m msg] [-a author]`

現在のワークスペースのエージェントログからスナップショットを作成します。

```bash
noa snapshot create -m "add login feature" -a "agent-001"
```

### `noa snapshot list`

すべてのワークスペースにわたるスナップショットを一覧表示します。

### `noa snapshot diff <a> <b>`

2つのスナップショット間のファイルレベルでの差分を表示します。

```bash
noa snapshot diff noa_abc123 noa_def456
```

## `noa workspace <subcommand>`

### `noa workspace create <name> [--agent <id>]`

現在のHEADからフォークした新しいワークスペースを作成します。

### `noa workspace switch <name>`

アクティブなワークスペースを切り替えます（HEADを更新）。

### `noa workspace list`

すべてのワークスペースを一覧表示します。`*`がアクティブなものを示します。

### `noa workspace delete <name>`

ワークスペースを削除します（アクティブなワークスペースは削除できません）。

### `noa workspace merge <from>`

3方向マージを使用して別のワークスペースを現在のワークスペースにマージします。

```bash
noa workspace switch default
noa workspace merge feature-1
```

## `noa remote <subcommand>`

### `noa remote add <name> <url>`

リモートリポジトリを追加します。

### `noa remote remove <name>`

リモートを削除します。

### `noa remote list`

設定されているすべてのリモートを一覧表示します。

## `noa push [--remote name]`

リモートにプッシュします（未実装）。

## `noa pull [--remote name]`

リモートからプルします（未実装）。

## `noa fetch [--remote name]`

マージせずにリモートからフェッチします（未実装）。

## `noa clone <url> [path]`

リモートリポジトリをクローンします（未実装）。
