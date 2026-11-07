# アーキテクチャ概観

noa はエージェントネイティブな分散バージョン管理システムです。「git のラッパー」ではありません: コアのオブジェクトモデルは最初から SHA-256 内容アドレス指定でありながら、git のオブジェクト代数とバイト単位で互換性を保ちます（同じファイル内容は noa と git の両方で同じ blob OID にハッシュされます——ミラーの忠実度検証はまさにこの性質に依存しています）。

## 3 層モデル

```
Workspace          — one isolated workspace per agent, invisible to each other
   │ head points to
Snapshot           — content-addressed commit (SHA-256), DAG structure (multiple parents = merge)
   │ tree_hash points to
ObjectStore        — blob / tree / symlink / gitlink (blobs byte-compatible with git;
                     trees/commits use noa's own SHA-256 addressing)
```

- **ワークスペース**は noa の中心的な隔離プリミティブです。複数のエージェントが同じリポジトリ内に互いを上書きすることなく、それぞれのワークスペースと head を持ちます。`mirror.<job>.<branch>` のようなシステム名前空間もワークスペースの形で存在します。
- **スナップショット**はコミットに相当するものです: `tree_hash` + `parents[]` + `author` + `timestamp` + `message`。スナップショット ID は `noa_<sha256>` 形式です（内容アドレス指定。同じツリーハッシュの繰り返し取り込みは冪等です）。
- **参照**（RedbRefStore）: `HEAD`、`mirror/{job}/{branch}`、`mirror/{job}/tags/{tag}`、`mirror/{job}/pr/{N}` など、すべての参照行が redb 内でスナップショット ID を指します。

ストレージエンジンは単一ファイルの埋め込み KV ストア（redb）です。デプロイはディレクトリのコピーだけを意味し、外部データベースは不要です。

## サーバーコンポーネント（noa-server）

| コンポーネント | 責務 |
|---|---|
| REST API（`/api/v1/*`） | リポジトリ / PR / runs / ユーザー / ミラージョブの CRUD |
| git-http（`/git/{repo}/*`） | スマート HTTP: upload-pack（clone/fetch）+ receive-pack（push、W2a2 以降） |
| ミラーエンジン | 定期ウェーブ: 元から master / tags / PR head を fetch して noa へ取り込み。還流 push も可能 |
| 実行記録 | CI 実行記録の保存（chest DevOps パネルのデータソース） |
| 認証ミドルウェア | ルート単位の Bearer 認証 + ロールによる権限強制（REST）と can_write 強制（git push） |

サーバーの状態はすべて 1 つの redb データベースに収められており、サーバーは「ディレクトリがすべて」になるまでステートレスです。

## git 互換性の正確な境界

互換性は層ごとに正確に述べます:

- **blob: バイト単位で互換。** 生の内容そのものが git の blob ボディで、`git_blob_oid(content)` は git と一致します（ミラーの忠実度検証はまさにこれで blob 単位の対照を行います）
- **tree: 意味的には等価、ID は等価ではない。** path/kind/blob ごとの OID は git と正確に対応しますが、noa の tree ID は独自の SHA-256 内容アドレス指定であり、git のツリーオブジェクト形式ではありません
- **commit: noa 独自のモデル。** スナップショット ID は `noa_<sha256>` 形式で、git のコミットオブジェクトではありません

git 形態の表現は**ミラースクラッチ内の本物の git リポジトリ**に存在します。還流 push はスクラッチの git オブジェクトをそのまま使います（git から取り込んだコンテンツは変換ゼロ）。**noa ネイティブに作られたコンテンツ**（スクラッチを一度も経由しない純粋なワークスペーススナップショット）には git オブジェクト形態がなく、直接の還流にはスナップショット→git オブジェクトのシリアライザが必要です——これは既知の制限です（現在の実装では、この種の参照は還流 push には現れません）。

## ミラスクラッチの役割

`NOA_MIRROR_SCRATCH_DIR/{job}.git` はミラージョブごとのベアクローンで、三重の役割を持ちます:

1. **fetch 元のキャッシュ** — ウェーブが元から fetch し、インポーターはここから git オブジェクトを読みます
2. **git-http の提供本体** — `/git/{repo}` はこれに対して直接 upload-pack / receive-pack を実行します
3. **還流 push のソース** — 還流レーンはローカル参照を元へアトミックに push します

push（receive-pack）はスクラッチのローカル参照を更新し、次のウェーブがそれを noa の参照システムへ取り込み、還流レーンが元へ push し返します——3 つの経路が 1 つのスクラッチを共有し、ループが閉じます。

## ドキュメント一覧

- [クイックスタート](./getting-started.md)
- [ミラーと還流](./mirror.md)
- [権限とユーザー](./rbac.md)
- [git-http プロトコル](./git-http.md)
- [マージと競合](./merge.md)
- [CI 実行記録](./ci-runs.md)
- [HTTP API リファレンス](../reference/http-api.md)
- [CLI リファレンス](../reference/cli.md)
- [デプロイ](../reference/deploy.md)
