# HTTP API 參考

基底位址：`http://<host>:<port>/api/v1`。全部端點都要求 bearer 驗證（`Authorization: Bearer
<token>`）；未驗證回 401 + Basic challenge。角色強制執行：reader 的寫入請求回 403；`/users` 非
GET 僅限 admin。

錯誤形狀統一為 `{"error": "..."}`；分頁以行內方式進行（`?limit=` 1..=1000、`?offset=`）。

## repos — 儲存庫目錄

| 方法 | 路徑 | 說明 |
|---|---|---|
| GET | `/repos` | 列出 catalog |
| POST | `/repos` | 註冊（`name` 必填，形狀同鏡像 job 名） |
| GET | `/repos/{name}` | 單一 |
| DELETE | `/repos/{name}` | 刪除（鏡像 job 存活或有 open PR 時回 409） |
| GET | `/repos/{name}/prs` | 儲存庫作用域的 PR 列表 |
| POST/GET | `/repos/{name}/runs` | CI 執行紀錄（見 [CI 執行紀錄](../guide/ci-runs.md)） |

## prs — Pull Request

| 方法 | 路徑 | 說明 |
|---|---|---|
| GET | `/prs?state=&base=&repo=&limit=&offset=` | 全域列表（`state` ∈ open/closed/merged） |
| POST | `/prs` | 建立（`title`、`repo`、`base`、`head` 等） |
| GET | `/prs/{number}` | 單一 |
| POST | `/prs/{number}/merge` | 合併（squash/快進；衝突時回 409 並附衝突清單） |

儲存庫作用域變體：`GET /repos/{name}/prs`——由路徑決定儲存庫，`?repo=` 篩選與之矛盾時回 400。

## users — 使用者與角色（admin）

| 方法 | 路徑 | 說明 |
|---|---|---|
| GET | `/users` | 列表（admin） |
| POST | `/users` | 建立/更新（同名 = 輪替 token 或改角色；`created_us` 保留） |
| GET | `/users/{name}` | 單一（admin） |
| DELETE | `/users/{name}` | 刪除（admin，不可逆） |

角色：`admin`（全部）/ `operator`（讀寫）/ `reader`（唯讀）。欄位與校驗見
[權限與使用者](../guide/rbac.md)。

## mirror — 鏡像任務

| 方法 | 路徑 | 說明 |
|---|---|---|
| GET | `/mirror/jobs` | 列出全部任務（含 `last_status` / `last_lane` / `last_detail`） |
| POST | `/mirror/jobs` | 建立 / 更新（同名即校驗 + 全量重置） |
| DELETE | `/mirror/jobs/{name}` | 刪除任務 |
| POST | `/mirror/run` | 手動觸發（body 為 `{"job":"<name>"}`，202 非同步執行） |

欄位、校驗與車道語意見 [鏡像與回流](../guide/mirror.md)。

## git-http — 智慧協定

`/git/{repo}/*`：clone/fetch（upload-pack）與 push（receive-pack）。驗證、行為細節與
邊界見 [git-http 協定](../guide/git-http.md)。

## 通用約定

- 所有列表端點的 `limit` 預設 50、上限 1000（`limit=0` → 400）；
- 分頁參數**行內**放在查詢字串（無巢狀包裹物件）；
- 伺服器端自訂欄位（如 `number`、`started_us`）由伺服器端賦值，用戶端提供的值會被覆寫；
- 錯誤不洩漏內部細節（`internal server error` 常數形狀），細節進到伺服器端日誌。
