# CI 執行紀錄（runs API）

noa 內建 CI 執行紀錄儲存：一次「對某儲存庫某 head 執行的檢查流程」就是一筆 run。這是
chest DevOps 面板 CI 檢視 `CI_BACKEND=noa` 車道的資料來源。

## 端點

| 方法 | 路徑 | 說明 |
|---|---|---|
| POST | `/api/v1/repos/{name}/runs` | 記錄一筆 run（寫入角色） |
| GET | `/api/v1/repos/{name}/runs?limit=&offset=` | 列表，最新的在前 |

- `{name}` 必須存在於 catalog 中（否則 404）；
- `number` 由伺服器端按儲存庫分配（原子性：掃描與寫入在同一個寫入交易內，並行記錄不會同號互覆）；
- 校驗：`head_sha` 為 40 個 hex；`status` ∈ `queued | running | success | failure | cancelled`；
  `kind` 1..=64 個字元；`actor` 1..=200；`log_ref` / `summary` ≤ 2000。

## 記錄一筆 run

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

回應（201）：伺服器端補齊 `repo`、`number`、`started_us`（用戶端未提供時）。

## 查詢

```bash
curl -H "Authorization: Bearer $TOKEN" \
  "http://127.0.0.1:3016/api/v1/repos/noa/runs?limit=10&offset=0"
```

回傳的資料列形如：

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

## 設計邊界

- **日誌不存 noa**：記錄攜帶不透明的 `log_ref` 指標，指向排程管線的日誌儲存；
- **資料列內容有上限**（見校驗）——防止已驗證的寫入者膨脹資料列體積；
- **儲存**：redb `runs` 表，鍵為 `{repo}#{number:020}`（字典序 = 儲存庫內時間序）。

## chest 整合

chest 的 `ci_ops` 處理器讀取 `CI_BACKEND` 環境變數：

- `github`（預設）：v1 GitHub Actions 車道，行為與歷史版本完全一致；
- `noa`：讀取 `NOA_API_BASE` + `NOA_API_TOKEN` 指向的 noa runs API，資料列正規化為與
  GitHub 資料列相同的面板形狀（`kind`→name、`number`→run_number、`started_us`→RFC3339
  `created_at`、終態映射 conclusion）；watch 列表使用 `CI_NOA_REPOS`（單段目錄名），未設
  定時回退 `CI_GITHUB_REPOS` 的末段。

面板的 `ci.config.get` 會回報 `backend` 欄位；`configured` 按 backend 感知——pure-noa
部署（無 GitHub token）只要 noa 三元組齊備即視為已連線。
