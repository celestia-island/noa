# CI 執行紀錄（runs API）

noa 内建 CI 執行紀錄存储：一次「对某仓某 head 執行的检查流水」就是一条 run。这是
chest DevOps 面板 CI 视圖 `CI_BACKEND=noa` 车道的數據源。

## 端點

| 方法 | 路径 | 说明 |
|---|---|---|
| POST | `/api/v1/repos/{name}/runs` | 記錄一条 run（寫角色） |
| GET | `/api/v1/repos/{name}/runs?limit=&offset=` | 列表，最新在前 |

- `{name}` 必须在 catalog 中（否則 404）；
- `number` 由伺服器端按仓分配（原子：扫描與寫入同一寫事務，并發記錄不会同号互覆）；
- 校驗：`head_sha` 40 hex；`status` ∈ `queued | running | success | failure | cancelled`；
  `kind` 1..=64 字符；`actor` 1..=200；`log_ref` / `summary` ≤ 2000。

## 記錄一条 run

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

響应（201）：伺服器端补齐 `repo`、`number`、`started_us`（用戶端未提供时）。

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

## 設计边界

- **日誌不存 noa**：記錄携带不透明 `log_ref` 指针，指向調度管線的日誌存储；
- **行内容有上限**（见校驗）——防止认證寫者膨胀行體积；
- **存储**：redb `runs` 表，鍵 `{repo}#{number:020}`（字典序 = 仓内时间序）。

## chest 集成

chest 的 `ci_ops` 处理器讀取 `CI_BACKEND` 環境變數：

- `github`（預設）：v1 GitHub Actions 车道，行为與歷史版本完全一致；
- `noa`：讀取 `NOA_API_BASE` + `NOA_API_TOKEN` 指向的 noa runs API，行歸一到與 GitHub
  行相同的面板形状（`kind`→name、`number`→run_number、`started_us`→RFC3339
  `created_at`、終態映射 conclusion）；watch 列表用 `CI_NOA_REPOS`（單段目錄名），未設
  时回退 `CI_GITHUB_REPOS` 的末段。

面板 `ci.config.get` 报告 `backend` 字段；`configured` 按 backend 感知——pure-noa
部署（无 GitHub token）只要 noa 三元組齐备即视为已連接。
