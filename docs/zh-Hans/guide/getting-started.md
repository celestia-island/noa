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

## noa 原生工作流（推荐）

`noa` CLI 是第一等入口——clone 下来的是 `.git` 与 `.noa` 共存的仓（git 管源码、noa 管智能体数据）：

```bash
noa clone http://<token>@127.0.0.1:3016/git/hello --path hello
cd hello
noa workspace create agent-a     # 按智能体隔离的工作区
# … 编辑文件 …
noa snapshot create -m "改动" -a agent-a
noa log                          # 本工作区的快照历史
noa push                         # 发布当前工作区快照（自动序列化为 git 提交推送）
```

`noa push` 发布的是**工作区快照态**（生成 `[noa export]` 提交），不是 `.git` 里的提交——两条车道并行，别混用（先 `noa snapshot create` 再 push，否则会推旧快照）。认证用 `NOA_GIT_USER` + `NOA_GIT_PASS` 环境变量注入。整个目录（含 `.git/noa`）拷到别的机器照常工作。

## git 兼容车道（任何 git 工具都能用）

同样的 `/git/{repo}` 端点也说标准 git 智能 HTTP——stock git、CI、farm 无感可用：

```bash
git clone http://<token>@127.0.0.1:3016/git/hello
```

推送（receive-pack）需要可写角色；落地的推送会**立即触发导入波**（下一波回流 origin）。这就是 farm/CNB 零改动的过渡形态。

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

## 备份与迁移

服务端全部状态可一键导出为可携带目录（详见[部署](./../reference/deploy.md)的备份一节）：
redb 一致快照 + 每仓一个 git bundle + sha256 清单；`noa-server --import-state <dir>` 离线恢复，
恢复演练 = 导出 → 异机恢复 → 起服对账。

## 下一步

- [架构总览](./architecture.md) —— 工作区 / 快照 / 引用模型
- [镜像与回流](./mirror.md) —— 双车道、PR heads、回流与 canary
- [HTTP API 参考](../reference/http-api.md) —— 全部端点
