# 快速上手

本頁帶你從零把 noa 跑起來：建置、啟動伺服器、完成第一次 clone / push。

## 前置條件

- Rust stable ≥ 1.99（以 `rustup` 安裝）
- git ≥ 2.39（伺服器端的 smart HTTP 直驅 `upload-pack` / `receive-pack`）
- 選用：node + pnpm（建置 WebUI）

## 建置

```bash
git clone https://github.com/celestia-island/noa.git
cd noa
cargo build --release --locked
```

產物：`target/release/noa`（CLI）與 `target/release/noa-server`（伺服器端）。

## 啟動伺服器

最小設定只需一個 API token（管理用的引導憑證）：

```bash
export NOA_API_TOKEN="<your-token>"
export NOA_MIRROR_SCRATCH_DIR=/var/lib/noa/mirror-cache   # mirror scratch root
export NOA_MIRROR_ENABLED=1                               # enable mirror scheduling (default 900s)
export NOA_MIRROR_GH_TOKEN="<github-pat>"                 # optional: back-flow / canary push credential
noa-server --host 0.0.0.0 --port 3016
```

啟動後：

```bash
curl -H "Authorization: Bearer <your-token>" http://127.0.0.1:3016/api/v1/repos
```

回傳 `[]` 即代表伺服器就緒。所有 REST 端點都要求 bearer 驗證；`/git/{repo}/*` 的讀取路徑同理
（未驗證的請求會收到 401 與 `Basic` challenge）。

## noa 原生工作流（推薦）

`noa` CLI 是第一等入口——clone 下來的是 `.git` 與 `.noa` 共存的儲存庫（git 管原始碼、noa 管智慧型代理資料）：

```bash
noa clone http://<token>@127.0.0.1:3016/git/hello --path hello
cd hello
noa workspace create agent-a     # a per-agent isolated workspace
# … edit files …
noa snapshot create -m "change" -a agent-a
noa log                          # this workspace's snapshot history
noa push                         # publishes the workspace snapshot (serialized to a git commit)
```

`noa push` 發布的是**工作區快照態**（產生 `[noa export]` 提交），不是 `.git` 裡的提交——兩條車道並行，別混用（先 `noa snapshot create` 再 push，否則會推舊快照）。驗證用 `NOA_GIT_USER` + `NOA_GIT_PASS` 環境變數注入。整個目錄（含 `.git/noa`）拷到別台機器照常運作。

## git 相容車道（任何 git 工具都能用）

同樣的 `/git/{repo}` 端點也說標準 git smart HTTP——stock git、CI、farm 無感可用：

```bash
git clone http://<token>@127.0.0.1:3016/git/hello
```

推送（receive-pack）需要可寫角色；落地的推送會**立即觸發匯入波**（回流在同一波次接續）。這就是 farm/CNB 零改動的過渡形態。

## 第一次鏡像（把一個 GitHub 儲存庫納入 noa）

```bash
curl -X POST -H "Authorization: Bearer <your-token>" \
  -H "Content-Type: application/json" \
  -d '{
    "name": "hello",
    "remote_url": "https://github.com/you/hello.git",
    "github_slug": "you/hello",
    "branches": ["master"],
    "tags": true,
    "pr_heads": true,
    "enabled": true
  }' \
  http://127.0.0.1:3016/api/v1/mirror/jobs

curl -X POST -H "Authorization: Bearer <your-token>" \
  -H "Content-Type: application/json" \
  -d '{"job":"hello"}' \
  http://127.0.0.1:3016/api/v1/mirror/run
```

波次完成後，該儲存庫即可透過 noa 進行 clone：

```bash
git clone http://<your-token>@127.0.0.1:3016/git/hello
```

## 備份與遷移

伺服器全部狀態可匯出為單一可攜目錄（詳見[部署](../reference/deploy.md)的備份一節）：
redb 一致快照 + 每儲存庫一個 git bundle + sha256 清單；`noa-server --import-state <dir>` 離線還原，
驗收標準是演練：匯出 → 異機還原 → 起服對帳。


## 下一步

- [架構總覽](./architecture.md) —— 工作區 / 快照 / 引用模型
- [鏡像與回流](./mirror.md) —— 雙車道、PR heads、回流與 canary
- [HTTP API 參考](../reference/http-api.md) —— 全部端點
