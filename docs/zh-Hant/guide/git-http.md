# git-http 協定

`/git/{repo}/*` 提供智慧 HTTP 協定：讀取路徑直驅 `git upload-pack --stateless-rpc`，
寫入路徑（自 W2a2 起）直驅 `git receive-pack --stateless-rpc`——不經 `git-http-backend`
CGI 包裝，因此協定協商、環境變數與錯誤形狀完全可控。

## 端點

| 方法 | 路徑 | 用途 |
|---|---|---|
| GET | `/git/{repo}/info/refs?service=git-upload-pack` | clone/fetch 通告 |
| POST | `/git/{repo}/git-upload-pack` | pack 協商 |
| GET | `/git/{repo}/info/refs?service=git-receive-pack` | push 通告 |
| POST | `/git/{repo}/git-receive-pack` | 接收推送 |

`{repo}` 必須是鏡像任務名——服務主體就是該任務的 scratch 裸儲存庫
（`NOA_MIRROR_SCRATCH_DIR/{repo}.git`）。儲存庫不存在回傳 404。

## 行為細節

- **協定協商**：用戶端的 `Git-Protocol` 標頭透傳給子行程（v2 按需啟用；空白時預設 v0）。
  這正是直驅實作取代 git-http-backend 的原因——CGI 包裝會在 POST 路徑丟棄該標頭，
  靜默降級為 v0 而弄壞 clone。
- **通告前導**：`# service=` pkt-line（長度標頭按服務名動態計算）+ flush；RPC 回應無前導。
- **Content-Type**：`application/x-git-{upload,receive}-pack[-advertisement]` 嚴格區分。
- **請求主體**：POST 內容直接寫入子行程 stdin；`Content-Encoding: gzip` 自動解壓縮
  （解壓比例 > 1000:1 即拒絕——炸彈防護；50 MiB 請求 / 200 MiB 解壓上限）。
- **逾時**：整個交換限時 60 秒牆鐘；逾時即終止子行程（kill_on_drop 防止孤兒行程持有鎖）。
- **並行**：以路由層級的號誌（semaphore）限制進行中的 git 交換數（記憶體上界 4×50 MiB）。
- **子行程環境**：`env_clear` 之後僅保留 `PATH` + `GIT_PROTOCOL`——環境洩漏在編譯期即不可能。

## 推送語意

推送落進 scratch 的本地引用；鏡像波次接著：

1. 把 scratch 的進階引用匯入 noa 引用體系（快照/工作區）；
2. `backflow` 開啟時把狀態推回 origin。

因此推送不需要任何特殊鉤子——三條車道（讀取、接收、回流）共享同一 scratch，閉迴路自動
收斂。推送要求已驗證身分；無憑證回 401 + Basic challenge，reader 角色回 403。

## 已知邊界

- 推送不提供刪除語意（noa 側刪除不回傳 origin——已知不對稱）。
- 不支援 dumb HTTP（`/info/refs` 裸檔案）。
- PR heads 回流落在 `refs/noa/pr/<N>`（GitHub 拒絕 `refs/pull/*` 推送）。
- 回流推送的來源是 scratch 的 git 物件——**noa 原生生產的提交**（純工作區快照，
  未經過 scratch）**不在回流範圍內**，直至快照 → git 物件序列化能力落地（見
  [架構總覽](./architecture.md)）。
