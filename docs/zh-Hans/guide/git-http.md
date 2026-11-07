# git-http 协议

`/git/{repo}/*` 提供智能 HTTP 协议：读路径直驱 `git upload-pack --stateless-rpc`，
写路径（W2a2 起）直驱 `git receive-pack --stateless-rpc`——不经 `git-http-backend`
CGI 包装，因此协议协商、环境变量与错误形状完全可控。

## 端点

| 方法 | 路径 | 用途 |
|---|---|---|
| GET | `/git/{repo}/info/refs?service=git-upload-pack` | clone/fetch 广告 |
| POST | `/git/{repo}/git-upload-pack` | pack 协商 |
| GET | `/git/{repo}/info/refs?service=git-receive-pack` | push 广告 |
| POST | `/git/{repo}/git-receive-pack` | 接收推送 |

`{repo}` 必须是镜像任务名——服务体就是该任务的 scratch 裸克隆
（`NOA_MIRROR_SCRATCH_DIR/{repo}.git`）。仓库不存在返回 404。

## 行为细节

- **协议协商**：客户端的 `Git-Protocol` 头透传给子进程（v2 按需启用；空默认 v0）。
  这正是直驱实现替代 git-http-backend 的原因——CGI 包装在 POST 路径丢弃该头，
  静默降级 v0 弄坏克隆。
- **广告前导**：`# service=` pkt-line（长度头按服务名动态计算）+ flush；RPC 响应无前导。
- **Content-Type**：`application/x-git-{upload,receive}-pack[-advertisement]` 严格区分。
- **请求体**：POST 体直接写子进程 stdin；`Content-Encoding: gzip` 自动解压
  （解压比例 > 1000:1 拒绝——炸弹防护；50 MiB 请求 / 200 MiB 解压上限）。
- **超时**：整个交换 60s 墙钟；超时杀子进程（kill_on_drop 防孤儿持锁）。
- **并发**：路由级信号量限制在飞 git 交换数（内存上界 4×50 MiB）。
- **子进程环境**：`env_clear` 后仅保留 `PATH` + `GIT_PROTOCOL`——环境泄漏在编译期不可能。

## 推送语义

推送落进 scratch 的本地引用；镜像波次随后：

1. 把 scratch 的高级引用导入 noa 引用体系（快照/工作区）；
2. `backflow` 开启时把状态推回 origin。

因此推送不需要任何特殊钩子——三条车道（读、收、回流）共享同一 scratch，闭环自动
收敛。推送要求已认证身份；无凭据 401 + Basic challenge，reader 角色 403。

## 已知边界

- 推送不提供删除语义（noa 侧删除不回传 origin——已知不对称）。
- dumb HTTP（`/info/refs` 裸文件）不服务。
- PR heads 回流落在 `refs/noa/pr/<N>`（GitHub 拒绝 `refs/pull/*` 推送）。
