# CI 运行记录（runs API）

noa 内建 CI 运行记录存储：一次「对某仓某 head 执行的检查流水」就是一条 run。这是
chest DevOps 面板 CI 视图 `CI_BACKEND=noa` 车道的数据源。

## 端点

| 方法 | 路径 | 说明 |
|---|---|---|
| POST | `/api/v1/repos/{name}/runs` | 记录一条 run（写角色） |
| GET | `/api/v1/repos/{name}/runs?limit=&offset=` | 列表，最新在前 |

- `{name}` 必须在 catalog 中（否则 404）；
- `number` 由服务端按仓分配（原子：扫描与写入同一写事务，并发记录不会同号互覆）；
- 校验：`head_sha` 40 hex；`status` ∈ `queued | running | success | failure | cancelled`；
  `kind` 1..=64 字符；`actor` 1..=200；`log_ref` / `summary` ≤ 2000。

## 记录一条 run

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

响应（201）：服务端补齐 `repo`、`number`、`started_us`（客户端未提供时）。

## 查询

```bash
curl -H "Authorization: Bearer $TOKEN" \
  "http://127.0.0.1:3016/api/v1/repos/noa/runs?limit=10&offset=0"
```

返回行形如：

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

## 设计边界

- **日志不存 noa**：记录携带不透明 `log_ref` 指针，指向调度管线的日志存储；
- **行内容有上限**（见校验）——防止认证写者膨胀行体积；
- **存储**：redb `runs` 表，键 `{repo}#{number:020}`（字典序 = 仓内时间序）。

## chest 集成

chest 的 `ci_ops` 处理器读取 `CI_BACKEND` 环境变量：

- `github`（默认）：v1 GitHub Actions 车道，行为与历史版本完全一致；
- `noa`：读取 `NOA_API_BASE` + `NOA_API_TOKEN` 指向的 noa runs API，行归一到与 GitHub
  行相同的面板形状（`kind`→name、`number`→run_number、`started_us`→RFC3339
  `created_at`、终态映射 conclusion）；watch 列表用 `CI_NOA_REPOS`（单段目录名），未设
  时回退 `CI_GITHUB_REPOS` 的末段。

面板 `ci.config.get` 报告 `backend` 字段；`configured` 按 backend 感知——pure-noa
部署（无 GitHub token）只要 noa 三元组齐备即视为已连接。
