# 快速上手

本页带你从零把 noa 跑起来：构建、启动服务端、完成第一次 clone / push。

## 前置条件

- Rust stable ≥ 1.99（`rustup` 安装）
- git ≥ 2.39（服务端的 smart HTTP 直驱 `upload-pack` / `receive-pack`）
- 可选：node + pnpm（构建 WebUI）

## 构建

```bash
git clone https://github.com/celestia-island/noa.git
cd noa
cargo build --release --locked
```

产物：`target/release/noa`（CLI）与 `target/release/noa-server`（服务端）。

## 启动服务端

最小配置只需一个 API token（管理引导凭据）：

```bash
export NOA_API_TOKEN="<your-token>"
export NOA_MIRROR_SCRATCH_DIR=/var/lib/noa/mirror-cache   # 镜像 scratch 根目录
export NOA_MIRROR_ENABLED=1                               # 启用镜像调度（默认 900s）
export NOA_MIRROR_GH_TOKEN="<github-pat>"                 # 可选：回流 / canary 推送凭据
noa-server --host 0.0.0.0 --port 3016
```

启动后：

```bash
curl -H "Authorization: Bearer <your-token>" http://127.0.0.1:3016/api/v1/repos
```

返回 `[]` 即服务就绪。所有 REST 端点都要求 bearer 认证；`/git/{repo}/*` 的读路径同理
（未认证请求收到 401 与 `Basic` challenge）。

## 第一次镜像（把一个 GitHub 仓收进 noa）

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

波次完成后，该仓即可通过 noa 克隆：

```bash
git clone http://<your-token>@127.0.0.1:3016/git/hello
```

## 第一次推送（写权威）

推送走 `/git/{repo}` 的 receive-pack 路径，需要已认证身份：

```bash
cd hello
echo "hello noa" > README.md
git add -A && git commit -m "hello noa"
git push origin master
```

推送落进镜像 scratch；下一个镜像波次会自动把它收进 noa 的快照/引用体系，若该 job 开启了
`backflow`，还会把状态推回 origin（GitHub）——这就是「noa 权威 + GitHub 镜像」的过渡形态。

## 下一步

- [架构总览](./architecture.md) —— 工作区 / 快照 / 引用模型
- [镜像与回流](./mirror.md) —— 双车道、PR heads、回流与 canary
- [HTTP API 参考](../reference/http-api.md) —— 全部端点
