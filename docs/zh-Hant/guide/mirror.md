# 鏡像與回流

鏡像系統把外部儲存庫（GitHub 等）的 master、全部 tags 與活躍 PR heads 納入 noa；回流車道
再把 noa 的狀態推回 origin——構成「noa 權威 + GitHub 鏡像」的過渡形態，farm/CNB 照舊
從 GitHub fetch，零改動。

## 鏡像任務（MirrorJob）

```json
{
  "name": "noa",
  "remote_url": "https://github.com/celestia-island/noa.git",
  "github_slug": "celestia-island/noa",
  "branches": ["master"],
  "tags": true,
  "pr_heads": true,
  "backflow": true,
  "canary_push": true,
  "enabled": true
}
```

| 欄位 | 說明 |
|---|---|
| `name` | `^[a-z0-9][a-z0-9-]{0,63}$`；同時是 scratch 目錄名、catalog 儲存庫名、clone 路徑段 |
| `remote_url` | 僅允許 `https://` / `file://`；禁止內嵌憑證（`@`、`?`） |
| `github_slug` | `owner/repo` 形狀校驗；Git Data API 旁路與回流使用 |
| `branches` | 至少一個；`tags`/`tags/*`、`pr/*`（PR 命名空間衝突）與衍生工作區的不合法形狀都會被拒絕 |
| `tags` | 鏡像全部 tags |
| `pr_heads` | 鏡像活躍 PR heads（`refs/pull/<N>/head` → `mirror/{job}/pr/<N>`） |
| `backflow` | 每波把 scratch 狀態推回 origin（詳見下文） |
| `canary_push` | 推一枚探針引用以驗證寫入方向 |
| `enabled` | 關閉後排程器會跳過 |

CRUD：`POST/GET /api/v1/mirror/jobs`、`DELETE /api/v1/mirror/jobs/{name}`、
`POST /api/v1/mirror/run`（手動觸發，body 為 `{"job":"<name>"}`）。

## 雙車道

- **git 車道**：scratch 裸儲存庫 + `git fetch`——tags、PR heads、回流推送都走它。
- **API 車道**：git 傳輸失效時（網路抖動族）自動降級為 Git Data API 逐 blob 匯入，
  每個 blob 都校驗 SHA（忠實度不因車道而降級）。

降級是波次層級的：`ensure_scratch` 失敗 → 該波改走 API 車道 → 自我修復刪除半壞的 scratch，
下一波重新 clone。回流推送只在 git 車道可用時執行（API 無法推送引用）。

## PR heads

`pr_heads: true` 時每波額外擷取 `refs/pull/<N>/head`，匯入為
`mirror/{job}/pr/<N>` + 同名工作區。PR 命名空間與分支命名空間（`pr/<x>` 形狀的分支）
互斥，upsert 時會拒絕。

## 回流（backflow）

```text
scratch 本地引用 ──atomic push──▶ origin
  +refs/heads/*:refs/heads/*
  +refs/tags/*:refs/tags/*
  +refs/pull-heads/*:refs/noa/pr/*     （僅當 scratch 存在 pull-heads）
```

- **憑證**：`token_env`（預設 `NOA_MIRROR_GH_TOKEN`）解析出的 token 以
  `x-access-token:<token>` 形式嵌入一次性 push URL；token 不會落入 scratch 設定。
- **force 語意**：翻轉後 noa 是寫入權威，origin 的引用向 scratch 收斂。刪除的不對稱
  （noa 側刪除不會刪除 origin 側引用）是已知限制。
- **失敗語意**：回流失敗會把波次標記為 FAILED——這是驗收關鍵車道，必須顯眼。
- **命名空間橋接**：GitHub 拒絕推送 `refs/pull/*`，因此 PR heads 落在
  `refs/noa/pr/<N>`。

## canary 探針

`canary_push: true` 時每波向 origin 推一枚 `noa-mirror-canary` 探針引用（首個已鏡像
分支的 commit），先走 git 傳輸、失敗則降級為 Git Data API——用於持續驗證寫入方向的
憑證與連線能力。探針失敗會讓波次響亮地失敗。

## 忠實度校驗

每個匯入的 commit 都做逐項對帳：路徑集合、條目 kind、blob OID（`git_blob_oid` 與 git
一致）必須完全相符；任何差異都會讓該項失敗並進入波次錯誤詳情。回流依賴這一不變量——
noa 物件與 git 物件一一對應，推送無需轉換層。

## 過期引用清理（reconcile）

tags 與 PR heads 的上游集合就是 reconcile 集：不再出現在上游的引用會被清出 noa
（`mirror/{job}/tags/`、`mirror/{job}/pr/` 命名空間）。回流的 tags 推送同樣以上游
為準（noa 側刪除不會刪除 origin 側——已知的不對稱）。

## 排程

`NOA_MIRROR_ENABLED=1` 時排程器每 900 秒跑一輪全部 enabled 任務；`POST /mirror/run`
可手動觸發。波次詳情（車道、各項結果、backflow/canary 狀態）在
`GET /api/v1/mirror/jobs` 的 `last_detail` / `last_lane`。
