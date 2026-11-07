# CI 実行記録（runs API）

noa は CI 実行記録のストレージを内蔵しています：「あるリポジトリのある head に対して実行されたチェック一連」が 1 件の run です。これは chest DevOps パネルの CI ビューにおける `CI_BACKEND=noa` レーンのデータソースです。

## エンドポイント

| メソッド | パス | 説明 |
|---|---|---|
| POST | `/api/v1/repos/{name}/runs` | run を 1 件記録（書き込みロール） |
| GET | `/api/v1/repos/{name}/runs?limit=&offset=` | 一覧、新しい順 |

- `{name}` は catalog に存在しなければなりません（そうでなければ 404）；
- `number` はサーバーがリポジトリごとに採番します（アトミック：スキャンと書き込みが同一の書き込みトランザクション内で行われ、並行記録が同番で互いを上書きすることはありません）；
- 検証：`head_sha` は 40 hex。`status` ∈ `queued | running | success | failure | cancelled`。`kind` は 1..=64 文字。`actor` は 1..=200。`log_ref` / `summary` は ≤ 2000。

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

応答（201）：サーバーが `repo`、`number`、`started_us`（クライアントが未指定の場合）を補完します。

## 照会

```bash
curl -H "Authorization: Bearer $TOKEN" \
  "http://127.0.0.1:3016/api/v1/repos/noa/runs?limit=10&offset=0"
```

返却行は次の形です：

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

## 設計境界

- **ログは noa に保存しない**：記録は不透明な `log_ref` ポインタを運び、スケジューリングパイプラインのログストレージを指します；
- **行内容には上限があります**（検証を参照）——認証済み書き手が行サイズを膨らませるのを防ぎます；
- **ストレージ**：redb の `runs` テーブル、キーは `{repo}#{number:020}`（辞書順 = リポジトリ内の時系列）。

## chest 統合

chest の `ci_ops` ハンドラは `CI_BACKEND` 環境変数を読み取ります：

- `github`（デフォルト）：v1 GitHub Actions レーン。挙動は過去バージョンと完全に一致します；
- `noa`：`NOA_API_BASE` + `NOA_API_TOKEN` が指す noa runs API を読み取り、行を GitHub 行と同じパネル形状へ正規化します（`kind`→name、`number`→run_number、`started_us`→RFC3339 の `created_at`、終状態は conclusion へマッピング）。watch リストは `CI_NOA_REPOS`（単一セグメントのディレクトリ名）を使用し、未設定なら `CI_GITHUB_REPOS` の末尾セグメントへフォールバックします。

パネルの `ci.config.get` は `backend` フィールドを報告します。`configured` は backend ごとに判定します——pure-noa デプロイ（GitHub トークンなし）では、noa の 3 つ組が揃っていれば接続済みとみなします。
