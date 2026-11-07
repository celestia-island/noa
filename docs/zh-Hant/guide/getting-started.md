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
export NOA_MIRROR_SCRATCH_DIR=/var/lib/noa/mirror-cache   # 鏡像 scratch 根目錄
export NOA_MIRROR_ENABLED=1                               # 啟用鏡像排程（預設 900s）
export NOA_MIRROR_GH_TOKEN="<github-pat>"                 # 選用：回流 / canary 推送憑證
noa-server --host 0.0.0.0 --port 3016
```

啟動後：

```bash
curl -H "Authorization: Bearer <your-token>" http://127.0.0.1:3016/api/v1/repos
```

回傳 `[]` 即代表伺服器就緒。所有 REST 端點都要求 bearer 驗證；`/git/{repo}/*` 的讀取路徑同理
（未驗證的請求會收到 401 與 `Basic` challenge）。

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

## 第一次推送（寫入權威）

推送走 `/git/{repo}` 的 receive-pack 路徑，需要已驗證的身分：

```bash
cd hello
echo "hello noa" > README.md
git add -A && git commit -m "hello noa"
git push origin master
```

推送落進鏡像 scratch；下一個鏡像波次會自動把它納入 noa 的快照/引用體系，若該 job 開啟了
`backflow`，還會把狀態推回 origin（GitHub）——這就是「noa 權威 + GitHub 鏡像」的過渡形態。

## 下一步

- [架構總覽](./architecture.md) —— 工作區 / 快照 / 引用模型
- [鏡像與回流](./mirror.md) —— 雙車道、PR heads、回流與 canary
- [HTTP API 參考](../reference/http-api.md) —— 全部端點
