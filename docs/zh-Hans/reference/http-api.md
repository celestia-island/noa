# HTTP API 参考

基址：`http://<host>:<port>/api/v1`。全部端点要求 bearer 认证（`Authorization: Bearer
<token>`）；未认证 401 + Basic challenge。角色执法：reader 的写请求 403；`/users` 非
GET 仅 admin。

错误形状统一为 `{"error": "..."}`；分页内联（`?limit=` 1..=1000、`?offset=`）。

## repos — 仓库目录

| 方法 | 路径 | 说明 |
|---|---|---|
| GET | `/repos` | 列出 catalog |
| POST | `/repos` | 注册（`name` 必填，形状同镜像 job 名） |
| GET | `/repos/{name}` | 单个 |
| DELETE | `/repos/{name}` | 删除（镜像 job 存活或 open PR 时 409） |
| GET | `/repos/{name}/prs` | 仓作用域 PR 列表 |
| POST/GET | `/repos/{name}/runs` | CI 运行记录（见 [CI 运行记录](../guide/ci-runs.md)） |

## prs — 拉取请求

| 方法 | 路径 | 说明 |
|---|---|---|
| GET | `/prs?state=&base=&repo=&limit=&offset=` | 全局列表（`state` ∈ open/closed/merged） |
| POST | `/prs` | 创建（title、repo、base、head 等） |
| GET | `/prs/{number}` | 单个 |
| POST | `/prs/{number}/merge` | 合并（squash/快进；冲突 409 带冲突清单） |

仓作用域变体：`GET /repos/{name}/prs`——路径决定仓，`?repo=` 过滤与之矛盾时 400。

## users — 用户与角色（admin）

| 方法 | 路径 | 说明 |
|---|---|---|
| GET | `/users` | 列表（admin） |
| POST | `/users` | 创建/更新（同名 = 轮换 token 或改角色；`created_us` 保留） |
| GET | `/users/{name}` | 单个（admin） |
| DELETE | `/users/{name}` | 删除（admin，不可逆） |

角色：`admin`（全部）/ `operator`（读写）/ `reader`（只读）。字段与校验见
[权限与用户](../guide/rbac.md)。

## mirror — 镜像任务

| 方法 | 路径 | 说明 |
|---|---|---|
| GET | `/mirror/jobs` | 列出全部任务（含 `last_status` / `last_lane` / `last_detail`） |
| POST | `/mirror/jobs` | 创建 / 更新（同名校验全量重置） |
| DELETE | `/mirror/jobs/{name}` | 删除任务 |
| POST | `/mirror/run` | 手动触发（body `{"job":"<name>"}`，202 异步执行） |

字段、校验与车道语义见[镜像与回流](../guide/mirror.md)。

## git-http — 智能协议

`/git/{repo}/*`：clone/fetch（upload-pack）与 push（receive-pack）。认证、行为细节与
边界见 [git-http 协议](../guide/git-http.md)。

## 通用约定

- 所有列表端点的 `limit` 缺省 50、上限 1000（`limit=0` → 400）；
- 分页参数**内联**在查询字符串（无嵌套包裹对象）；
- 服务端自定义字段（如 `number`、`started_us`）由服务端赋值，客户端提供值被覆写；
- 错误不泄露内部细节（`internal server error` 常量形状），细节进服务端日志。
