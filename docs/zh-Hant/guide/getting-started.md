# 快速上手

本页带你從零把 noa 跑起来：构建、啟动伺服器端、完成第一次 clone / push。

## 前定条件

- Rust stable ≥ 1.99（`rustup` 安装）
- git ≥ 2.39（伺服器端的 smart HTTP 直驱 `upload-pack` / `receive-pack`）
- 可选：node + pnpm（构建 WebUI）

## 构建

```bash
git clone https://github.com/celestia-island/noa.git
cd noa
cargo build --release --locked
```

产物：`target/release/noa`（CLI）與 `target/release/noa-server`（伺服器端）。

## 啟动伺服器端

最小設定只需一个 API token（管理引導憑證）：

```bash
export NOA_API_TOKEN="<your-token>"
export NOA_MIRROR_SCRATCH_DIR=/var/lib/noa/mirror-cache   # 鏡像 scratch 根目錄
export NOA_MIRROR_ENABLED=1                               # 啟用鏡像調度（預設 900s）
export NOA_MIRROR_GH_TOKEN="<github-pat>"                 # 可选：回流 / canary 推送憑證
noa-server --host 0.0.0.0 --port 3016
```

啟动後：

```bash
curl -H "Authorization: Bearer <your-token>" http://127.0.0.1:3016/api/v1/repos
```

返回 `[]` 即服務就绪。所有 REST 端點都要求 bearer 认證；`/git/{repo}/*` 的讀路径同理
（未认證请求收到 401 與 `Basic` challenge）。

## 第一次鏡像（把一个 GitHub 仓收进 noa）

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

波次完成後，该仓即可通过 noa 克隆：

```bash
git clone http://<your-token>@127.0.0.1:3016/git/hello
```

## 第一次推送（寫權威）

推送走 `/git/{repo}` 的 receive-pack 路径，需要已认證身份：

```bash
cd hello
echo "hello noa" > README.md
git add -A && git commit -m "hello noa"
git push origin master
```

推送落进鏡像 scratch；下一个鏡像波次会自动把它收进 noa 的快照/參照體系，若该 job 開啟了
`backflow`，还会把状態推回 origin（GitHub）——这就是「noa 權威 + GitHub 鏡像」的过渡形態。

## 下一步

- [架構總览](./architecture.md) —— 工作區 / 快照 / 參照模型
- [鏡像與回流](./mirror.md) —— 雙车道、PR heads、回流與 canary
- [HTTP API 参考](../reference/http-api.md) —— 全部端點
