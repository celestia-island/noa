# HTTP API リファレンス

ベース URL：`http://<host>:<port>/api/v1`。すべてのエンドポイントは bearer 認証を要求します（`Authorization: Bearer <token>`）。未認証は 401 + Basic challenge。ロール執行：reader の書き込みリクエストは 403。`/users` の非 GET は admin のみです。

エラー形状は `{"error": "..."}` に統一されています。ページネーションはインライン（`?limit=` 1..=1000、`?offset=`）。

## repos — リポジトリカタログ

| メソッド | パス | 説明 |
|---|---|---|
| GET | `/repos` | catalog を一覧表示 |
| POST | `/repos` | 登録（`name` 必須、形状はミラー job 名と同じ） |
| GET | `/repos/{name}` | 単一取得 |
| DELETE | `/repos/{name}` | 削除（ミラー job が生存中または open PR がある場合は 409） |
| GET | `/repos/{name}/prs` | リポジトリスコープの PR 一覧 |
| POST/GET | `/repos/{name}/runs` | CI 実行記録（[CI 実行記録](../guide/ci-runs.md) を参照） |

## prs — プルリクエスト

| メソッド | パス | 説明 |
|---|---|---|
| GET | `/prs?state=&base=&repo=&limit=&offset=` | グローバル一覧（`state` ∈ open/closed/merged） |
| POST | `/prs` | 作成（title、repo、base、head など） |
| GET | `/prs/{number}` | 単一取得 |
| POST | `/prs/{number}/merge` | マージ（squash/fast-forward。競合時は 409 + 競合リスト） |

リポジトリスコープのバリアント：`GET /repos/{name}/prs`——パスがリポジトリを決定し、`?repo=` フィルタと矛盾する場合は 400 を返します。

## users — ユーザーとロール（admin）

| メソッド | パス | 説明 |
|---|---|---|
| GET | `/users` | 一覧（admin） |
| POST | `/users` | 作成/更新（同名 = トークンのローテーションまたはロール変更。`created_us` は保持） |
| GET | `/users/{name}` | 単一取得（admin） |
| DELETE | `/users/{name}` | 削除（admin、不可逆） |

ロール：`admin`（すべて）/ `operator`（読み書き）/ `reader`（読み取りのみ）。フィールドと検証は [権限とユーザー](../guide/rbac.md) を参照してください。

## mirror — ミラージョブ

| メソッド | パス | 説明 |
|---|---|---|
| GET | `/mirror/jobs` | 全ジョブを一覧表示（`last_status` / `last_lane` / `last_detail` を含む） |
| POST | `/mirror/jobs` | 作成 / 更新（同名は検証のうえ全量リセット） |
| DELETE | `/mirror/jobs/{name}` | ジョブを削除 |
| POST | `/mirror/run` | 手動トリガー（body は `{"job":"<name>"}`、202 で非同期実行） |

フィールド・検証・レーンのセマンティクスは [ミラーとバックフロー](../guide/mirror.md) を参照してください。

## git-http — スマートプロトコル

`/git/{repo}/*`：clone/fetch（upload-pack）と push（receive-pack）。認証・動作詳細・境界は [git-http プロトコル](../guide/git-http.md) を参照してください。

## 共通規約

- すべての一覧エンドポイントの `limit` はデフォルト 50、上限 1000（`limit=0` → 400）；
- ページネーション パラメータはクエリ文字列に**インライン**（ネストしたラッパーオブジェクトなし）；
- サーバー側で採番するフィールド（`number`、`started_us` など）はサーバーが値を割り当て、クライアントが提供した値は上書きされます；
- エラーは内部詳細を漏らしません（`internal server error` の定数形状）。詳細はサーバーログへ記録されます。
