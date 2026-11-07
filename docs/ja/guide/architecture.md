# アーキテクチャ

noa は AI ネイティブな分散バージョン管理システムです。「git のラッパー」ではありません: コアのオブジェクトモデルは独自の SHA-256 内容アドレス指定を使いながら、git のオブジェクト代数とバイト単位で互換性を保ちます（同じファイル内容は noa と git で同じ blob OID を生みます——ミラーの忠実度検証はまさにこの性質に依存しています）。

## 3 層モデル

```
Workspace   —— one isolated workspace per agent; agents cannot see each other
   │ head points to
Snapshot    —— content-addressed commit (SHA-256), DAG structure (multi-parent = merge)
   │ tree_hash points to
ObjectStore —— blob / tree / symlink / gitlink, git-object-algebra compatible
```

- **ワークスペース**は noa の中心的な隔離プリミティブです。同じリポジトリ内の複数エージェントが、互いを上書きすることなく各自のワークスペースと head を持ちます。`mirror.<job>.<branch>` のようなシステム名前空間もワークスペースとして存在します。
- **スナップショット**はコミットに相当するものです: `tree_hash` + `parents[]` + `author` + `timestamp` + `message`。スナップショット ID は `noa_<sha256>` 形式です（内容アドレス指定。未変更ツリーの繰り返し取り込みは冪等です）。
- **参照**（RedbRefStore）: `HEAD`、`mirror/{job}/{branch}`、`mirror/{job}/tags/{tag}`、`mirror/{job}/pr/{N}`——redb 内のすべての参照行がスナップショット ID を指します。

ストレージエンジンは埋め込み型の単一ファイル KV ストア（redb）です。デプロイは「ディレクトリをコピーするだけ」で、外部データベースは不要です。

## サーバーコンポーネント（noa-server）

| コンポーネント | 責務 |
|---|---|
| REST API（`/api/v1/*`） | リポジトリ / PR / runs / ユーザー / ミラージョブの CRUD |
| git-http（`/git/{repo}/*`） | スマート HTTP: upload-pack（clone/fetch）+ receive-pack（push、W2a2 以降） |
| ミラーエンジン | 定期ウェーブ: 元から master / tags / PR head を noa へ pull。オプションで還流 push |
| 実行記録 | CI 実行記録の保存（chest DevOps パネルのデータソース） |
| 認証ミドルウェア | ルート単位の Bearer 認証 + ロールによる権限強制（REST）と can_write 強制（git push） |

サーバーの状態はすべて 1 つの redb データベースに収められています——「ディレクトリがすべて」です。

## git オブジェクト互換性

取り込みと還流はどちらも「git のオブジェクト代数は変わらない」という前提で動きます:

- blob: 生の内容がそのまま git の blob ボディです（`git_blob_oid(content)` が git と一致します）
- tree: 取り込み時、path/kind/oid をエントリ単位で突き合わせます（忠実度検証）
- commit: ミラーウェーブが親リンクを維持します（`noa_<sha>` の内容アドレス指定 + 親エッジ）

したがって noa → git の還流には変換レイヤーが一切不要です。スクラッチ内の git オブジェクトと noa のオブジェクトは 1 対 1 で対応します。

## ミラスクラッチの役割

`NOA_MIRROR_SCRATCH_DIR/{job}.git` はミラージョブごとのベアクローンで、3 つの役割を持ちます:

1. **pull 元のキャッシュ** — ウェーブが元から fetch し、インポーターはここから git オブジェクトを読みます
2. **git-http の提供本体** — `/git/{repo}` はこれに対して直接 upload-pack / receive-pack を実行します
3. **還流 push のソース** — 還流レーンはローカル参照を元へアトミックに push します

push（receive-pack）はスクラッチのローカル参照を更新し、次のウェーブがそれを noa の参照システムへ取り込み、還流レーンがその状態を元へ push します——3 つの経路が 1 つのスクラッチを共有し、ループは自分で閉じます。

## ドキュメント一覧

- [クイックスタート](./getting-started.md)
- [ミラーと還流](./mirror.md)
- [RBAC](./rbac.md)
- [git-http プロトコル](./git-http.md)
- [マージと競合](./merge.md)
- [CI 実行記録](./ci-runs.md)
- [HTTP API リファレンス](../reference/http-api.md)
- [CLI リファレンス](../reference/cli.md)
- [デプロイ](../reference/deploy.md)
