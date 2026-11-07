# CLI リファレンス

`noa` はワークスペース側のコマンドラインエントリポイントです（clap 派生。実際のシグネチャは `noa --help` で確認してください）。すべてのコマンドは `noa init` で初期化されたリポジトリ内で動作します。

## コマンド一覧

| コマンド | 目的 |
|---|---|
| `noa init` | 現在のディレクトリを noa リポジトリとして初期化する（`.noa` メタデータディレクトリを作成） |
| `noa status` | 現在のワークスペースの状態: head、未コミットの変更、ワークスペース一覧 |
| `noa log` | 現在のワークスペースのスナップショット履歴（切り詰めレンダリング + 親チェーンの辿り） |
| `noa snapshot` | スナップショット操作（作成 / 検査） |
| `noa workspace` | ワークスペース管理: `workspace create <name>`（エージェントごとの隔離）、切り替え、一覧 |
| `noa remote` | リモート管理（add / set-url / list） |
| `noa push` / `noa pull` / `noa fetch` / `noa clone` | リモートとの同期（git 互換のセマンティクス） |
| `noa resolve <path>` | ネストしたパスを tree エントリへ解決する |
| `noa coauthor` | コミットに共同作成者を付与する（エージェント協働の帰属） |
| `noa hook` | リポジトリフックの管理（センチネルマーク付き。既存フックを安全に上書き） |
| `noa verify` | 現在のワークスペースのエージェントログへ `verify.run/1` レコードを追記する（stdin または `--file` で JSON）——検証台帳の CLI ライター |
| `noa sync` | sync 操作 |
| `noa storage` | ストレージバックエンドの操作 |
| `noa pr` | PR ライフサイクル（forge 非依存）: `pr create` / `pr list` / `pr diff` など |

## ワークスペース隔離のワークフロー

```bash
noa init
noa workspace create agent-a      # agent A's isolated workspace
# … edit inside the workspace …
noa snapshot -m "agent-a's changes"
noa log                           # sees only this workspace's parent chain
```

複数のエージェントがそれぞれ `workspace create` を実行すれば、互いの head も内容も見えません——これが noa と伝統的なブランチワークフローの根本的な違いです: 隔離の単位は ref ではなくワークスペースです。

## サーバーとの連携

CLI の push/pull/fetch/clone はサーバーの `/git/*` スマートプロトコルと相互運用します（同じ git 互換のセマンティクス）。リモートを noa-server に向けると:

```bash
noa remote add origin http://noa.example.com:3016/git/noa
noa push origin master
```

push は receive-pack を通り、認証済みのアイデンティティを必要とします（トークンがパスワードを兼ねます。ロールは書き込み可能でなければなりません）。

## verify と台帳

`noa verify` は検証台帳の CLI ライターです: レコードは `verify.run/1` スキーマで、現在のワークスペースのエージェントログ（JSONL、追記専用）へ追記されます。レコードは `hardware=<hw-id>` のバインディングを運べます（ワークスペース側のハードウェアレジストリ監査と連携します）。

> 台帳の権威はファイル側（`/mnt/work/verify-ledger/`）にあります。CLI の書き込みはフェイルオープンの二重書き込みです——開けた書き手がチェーンを検証しないのは設計上の裁定であり、チェーンの検証は読み手側が行います。
