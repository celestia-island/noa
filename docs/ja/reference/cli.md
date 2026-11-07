# CLI リファレンス

`noa` はワークスペース側のコマンドライン入口です（clap 派生。`noa --help` で最新のシグネチャを確認できます）。すべてのコマンドは `noa init` 済みのリポジトリ内で実行します。

## コマンド一覧

| コマンド | 用途 |
|---|---|
| `noa init` | 現在のディレクトリを noa リポジトリとして初期化（`.noa` メタデータディレクトリを作成） |
| `noa status` | 現在のワークスペースの状態：head、未コミットの変更、ワークスペース一覧 |
| `noa log` | 現在のワークスペースのスナップショット履歴（切り詰めレンダリング + 親チェーンの走査） |
| `noa snapshot` | スナップショット操作（作成 / 表示） |
| `noa workspace` | ワークスペース管理：`workspace create <name>`（エージェントごとの隔離）、切り替え、一覧 |
| `noa remote` | リモート管理（add / set-url / list） |
| `noa push` / `noa pull` / `noa fetch` / `noa clone` | リモートとの同期（git 互換セマンティクス） |
| `noa resolve <path>` | ネストしたパスをツリーエントリへ解決 |
| `noa coauthor` | コミットにコオーサーを付与（エージェント協働の署名） |
| `noa hook` | リポジトリフックの管理（哨兵マーカー付きで既存フックを安全に上書き） |
| `noa verify` | 現在のワークスペースのエージェントログへ `verify.run/1` レコードを追記（JSON は stdin または `--file`）——検証台帳の CLI 書き手 |
| `noa sync` | 同期操作 |
| `noa storage` | ストレージバックエンド操作 |
| `noa pr` | PR ライフサイクル（forge 非依存）：`pr create` / `pr list` / `pr diff` など |

## ワークスペース隔離のワークフロー

```bash
noa init
noa workspace create agent-a      # 智能体 A 的隔离工作区
# … 在工作区内编辑 …
noa snapshot -m "agent-a 的变更"
noa log                           # 只看到本工作区的父链
```

複数のエージェントがそれぞれ `workspace create` を行い、互いの head と内容は不可視です——これが noa と従来のブランチワークフローの根本的な違いです：隔離の単位は参照ではなくワークスペースです。

## サーバーとの協調

CLI の push/pull/fetch/clone はサーバーの `/git/*` スマートプロトコルと相互運用します（git 互換セマンティクス）。リモートを noa-server に向けると：

```bash
noa remote add origin http://noa.example.com:3016/git/noa
noa push origin master
```

プッシュは receive-pack を通るため、認証済みアイデンティティが必要です（トークンがパスワードの役割を果たします。ロールは書き込み可能であること）。

## verify と台帳

`noa verify` は検証台帳の CLI 書き手です：レコードは `verify.run/1` スキーマで現在のワークスペースのエージェントログ（JSONL、追記専用）へ追記されます。レコードは `hardware=<hw-id>` バインディングを運べます（ワークスペース側のハードウェアレジストリ監査と組み合わせます）。

> 権威ある台帳はファイル側にあります（`/mnt/work/verify-ledger/`）。CLI による書き込みは fail-open の二重書き込みです——開かれた書き手がチェーンを検証しないのは設計上の判断であり、チェーン検証は読み取り側が行います。
