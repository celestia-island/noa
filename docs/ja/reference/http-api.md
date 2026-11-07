# HTTP API リファレンス

ベース URL: `http://<host>:<port>/api/v1`。すべてのエンドポイントは Bearer 認証を要求します（`Authorization: Bearer <token>`）。認証のないリクエストは 401 + `Basic` チャレンジを受け取ります。ロールの強制: reader の書き込みリクエストは 403。`/users` への非 GET は admin のみです。

エラーは一様に `{"error": "..."}` の形状です。ページネーションはインラインです（`?limit=` 1..=1000、`?offset=`）。

## repos — リポジトリカタログ

| メソッド | パス | 説明 |
|---|---|---|
| GET | `/repos` | カタログの一覧 |
| POST | `/repos` | 登録（`name` 必須。ミラージョブ名と同じ形式） |
| GET | `/repos/{name}` | 単一取得 |
| DELETE | `/repos/{name}` | 削除（ミラージョブが生きている、またはオープンな PR がある間は 409） |
| GET | `/repos/{name}/prs` | リポジトリ単位の PR 一覧 |
| POST/GET | `/repos/{name}/runs` | CI 実行記録（[CI 実行記録](../guide/ci-runs.md) を参照） |

## prs — プルリクエスト

| メソッド | パス | 説明 |
|---|---|---|
| GET | `/prs?state=&base=&repo=&limit=&offset=` | グローバル一覧（`state` ∈ open/closed/merged） |
| POST | `/prs` | 作成（title、repo、base、head など） |
| GET | `/prs/{number}` | 単一取得 |
| POST | `/prs/{number}/merge` | マージ（squash / fast-forward。競合時は競合一覧付きの 409） |

リポジトリ単位のバリアント: `GET /repos/{name}/prs`——リポジトリはパスが決定します。それと矛盾する `?repo=` フィルタは 400 を返します。

## users — ユーザーとロール（admin）

| メソッド | パス | 説明 |
|---|---|---|
| GET | `/users` | 一覧（admin） |
| POST | `/users` | 作成 / 更新（同名 = トークンのローテートまたはロール変更。`created_us` は保持） |
| GET | `/users/{name}` | 単一取得（admin） |
| DELETE | `/users/{name}` | 削除（admin、不可逆） |

ロール: `admin`（すべて）/ `operator`（読み書き）/ `reader`（読み取りのみ）。フィールドと検証: [権限とユーザー](../guide/rbac.md) を参照。

## mirror — ミラージョブ

| メソッド | パス | 説明 |
|---|---|---|
| GET | `/mirror/jobs` | すべてのジョブの一覧（`last_status` / `last_lane` / `last_detail` を含む） |
| POST | `/mirror/jobs` | 作成 / 更新（同名の検証を通るとジョブ全体をリセット） |
| DELETE | `/mirror/jobs/{name}` | ジョブの削除 |
| POST | `/mirror/run` | 手動トリガー（ボディ `{"job":"<name>"}`、202 非同期） |

フィールド・検証・レーンのセマンティクス: [ミラーと還流](../guide/mirror.md) を参照。

## git-http — スマートプロトコル

`/git/{repo}/*`: clone/fetch（upload-pack）と push（receive-pack）。認証・動作の詳細・既知の境界: [git-http プロトコル](../guide/git-http.md) を参照。

## 共通の規約

- すべての一覧エンドポイントで `limit` のデフォルトは 50、上限は 1000（`limit=0` → 400）
- ページネーションのパラメータはクエリ文字列に**インライン**です（ネストしたラッパーオブジェクトはありません）
- サーバーが採番するフィールド（`number`、`started_us` など）はサーバーが設定します。クライアントが指定した値は上書きされます
- エラーは内部の詳細を漏らしません（定数の `internal server error` 形状）。詳細はサーバーログへ
