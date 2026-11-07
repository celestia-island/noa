# CI 実行記録（runs API）

noa には CI 実行記録の保存機能が組み込まれています: 「あるリポジトリのある head に対して実行されたチェックパイプラインの 1 回」が 1 run です。これは chest DevOps パネルの CI ビューにおける `CI_BACKEND=noa` レーンのデータソースです。

## エンドポイント

| メソッド | パス | 説明 |
|---|---|---|
| POST | `/api/v1/repos/{name}/runs` | 1 run を記録する（書き込みロール） |
| GET | `/api/v1/repos/{name}/runs?limit=&offset=` | 一覧。新しい順 |

- `{name}` はカタログに存在しなければなりません（なければ 404）
- `number` はサーバーがリポジトリごとに採番します（アトミック: スキャンと書き込みが同じ書き込みトランザクション内で行われるため、並行して記録しても番号が互いに上書きされることはありません）
- 検証: `head_sha` は 40 桁の 16 進数。`status` ∈ `queued | running | success | failure | cancelled`。`kind` は 1..=64 文字。`actor` は 1..=200。`log_ref` / `summary` は ≤ 2000

## run の記録

```bash
curl -X POST -H "Authorization: Bearer $TOKEN" -H "Content-Type: application/json" \
  -d '{
    "head_sha": "<40-hex>",
    "kind": "basic-check",
    "status": "success",
    "actor": "farm-runner-1",
    "log_ref": "https://ci.example.com/logs/123"
  }' \
  http://127.0.0.1:3016/api/v1/repos/noa/runs
```

レスポンス（201）: サーバーが `repo`、`number`、`started_us`（クライアントが指定しなかった場合）を埋めます。

## 照会

```bash
curl -H "Authorization: Bearer $TOKEN" \
  "http://127.0.0.1:3016/api/v1/repos/noa/runs?limit=10&offset=0"
```

返される行は次のような形です:

```json
{
  "repo": "noa",
  "number": 1,
  "head_sha": "…",
  "kind": "basic-check",
  "status": "success",
  "actor": "farm-runner-1",
  "started_us": 1794049465562618
}
```

## 設計上の境界

- **ログは noa に保存しません**: レコードは dispatch パイプラインのログストアへの不透明な `log_ref` ポインタだけを持ちます
- **行の内容には上限があります**（上記の検証を参照）——認証済み書き手による行サイズの肥大化を防ぎます
- **ストレージ**: redb の `runs` テーブル。キーは `{repo}#{number:020}`（辞書順 = リポジトリ内の時系列順）

## chest 統合

chest の `ci_ops` ハンドラは `CI_BACKEND` 環境変数を読みます:

- `github`（デフォルト）: v1 GitHub Actions レーン。過去のバージョンと動作は同一です
- `noa`: `NOA_API_BASE` + `NOA_API_TOKEN` が指す noa の runs API を読み、GitHub の行と同じパネル形状へ行を正規化します（`kind`→name、`number`→run_number、`started_us`→RFC3339 の `created_at`、終端ステータスは conclusion へマッピング）。ウォッチリストは `CI_NOA_REPOS`（単一セグメントのディレクトリ名）を使い、未設定の場合は `CI_GITHUB_REPOS` の末尾セグメントへフォールバックします

パネルの `ci.config.get` は `backend` フィールドを報告します。`configured` は backend を考慮します——純 noa デプロイ（GitHub トークンなし）でも、noa の 3 点が揃っていれば接続済みと判定されます。
