# HTTP API 参考

基址：`http://<host>:<port>/api/v1`。全部端點要求 bearer 认證（`Authorization: Bearer
<token>`）；未认證 401 + Basic challenge。角色執法：reader 的寫请求 403；`/users` 非
GET 仅 admin。

错誤形状统一为 `{"error": "..."}`；分页内联（`?limit=` 1..=1000、`?offset=`）。

## repos — 儲存庫目錄

| 方法 | 路径 | 说明 |
|---|---|---|
| GET | `/repos` | 列出 catalog |
| POST | `/repos` | 注册（`name` 必填，形状同鏡像 job 名） |
| GET | `/repos/{name}` | 單个 |
| DELETE | `/repos/{name}` | 刪除（鏡像 job 存活或 open PR 时 409） |
| GET | `/repos/{name}/prs` | 仓作用域 PR 列表 |
| POST/GET | `/repos/{name}/runs` | CI 執行紀錄（见 [CI 執行紀錄](../guide/ci-runs.md)） |

## prs — 拉取请求

| 方法 | 路径 | 说明 |
|---|---|---|
| GET | `/prs?state=&base=&repo=&limit=&offset=` | 全局列表（`state` ∈ open/closed/merged） |
| POST | `/prs` | 建立（title、repo、base、head 等） |
| GET | `/prs/{number}` | 單个 |
| POST | `/prs/{number}/merge` | 合併（squash/快进；衝突 409 带衝突清單） |

仓作用域變體：`GET /repos/{name}/prs`——路径决定仓，`?repo=` 过滤與之矛盾时 400。

## users — 用戶與角色（admin）

| 方法 | 路径 | 说明 |
|---|---|---|
| GET | `/users` | 列表（admin） |
| POST | `/users` | 建立/更新（同名 = 輪换 token 或改角色；`created_us` 保留） |
| GET | `/users/{name}` | 單个（admin） |
| DELETE | `/users/{name}` | 刪除（admin，不可逆） |

角色：`admin`（全部）/ `operator`（讀寫）/ `reader`（只讀）。字段與校驗见
[權限與用戶](../guide/rbac.md)。

## mirror — 鏡像任務

| 方法 | 路径 | 说明 |
|---|---|---|
| GET | `/mirror/jobs` | 列出全部任務（含 `last_status` / `last_lane` / `last_detail`） |
| POST | `/mirror/jobs` | 建立 / 更新（同名校驗全量重定） |
| DELETE | `/mirror/jobs/{name}` | 刪除任務 |
| POST | `/mirror/run` | 手动觸發（body `{"job":"<name>"}`，202 異步執行） |

字段、校驗與车道語义见[鏡像與回流](../guide/mirror.md)。

## git-http — 智能协議

`/git/{repo}/*`：clone/fetch（upload-pack）與 push（receive-pack）。认證、行为細節與
边界见 [git-http 协議](../guide/git-http.md)。

## 通用约定

- 所有列表端點的 `limit` 缺省 50、上限 1000（`limit=0` → 400）；
- 分页参數**内联**在查询字符串（无嵌套包裹对象）；
- 伺服器端自定义字段（如 `number`、`started_us`）由伺服器端赋值，用戶端提供值被覆寫；
- 错誤不泄露内部細節（`internal server error` 常量形状），細節进伺服器端日誌。
