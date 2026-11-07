# git-http 协議

`/git/{repo}/*` 提供智能 HTTP 协議：讀路径直驱 `git upload-pack --stateless-rpc`，
寫路径（W2a2 起）直驱 `git receive-pack --stateless-rpc`——不经 `git-http-backend`
CGI 包装，因此协議协商、環境變數與错誤形状完全可控。

## 端點

| 方法 | 路径 | 用途 |
|---|---|---|
| GET | `/git/{repo}/info/refs?service=git-upload-pack` | clone/fetch 广告 |
| POST | `/git/{repo}/git-upload-pack` | pack 协商 |
| GET | `/git/{repo}/info/refs?service=git-receive-pack` | push 广告 |
| POST | `/git/{repo}/git-receive-pack` | 接收推送 |

`{repo}` 必须是鏡像任務名——服務體就是该任務的 scratch 裸克隆
（`NOA_MIRROR_SCRATCH_DIR/{repo}.git`）。儲存庫不存在返回 404。

## 行为細節

- **协議协商**：用戶端的 `Git-Protocol` 头透傳给子进程（v2 按需啟用；空預設 v0）。
  这正是直驱實现替代 git-http-backend 的原因——CGI 包装在 POST 路径丢棄该头，
  静默降級 v0 弄坏克隆。
- **广告前導**：`# service=` pkt-line（长度头按服務名动態计算）+ flush；RPC 響应无前導。
- **Content-Type**：`application/x-git-{upload,receive}-pack[-advertisement]` 严格区分。
- **请求體**：POST 體直接寫子进程 stdin；`Content-Encoding: gzip` 自动解压
  （解压比例 > 1000:1 拒绝——炸弹防护；50 MiB 请求 / 200 MiB 解压上限）。
- **超时**：整个交换 60s 墙钟；超时杀子进程（kill_on_drop 防孤儿持鎖）。
- **并發**：路由級信号量限制在飞 git 交换數（内存上界 4×50 MiB）。
- **子进程環境**：`env_clear` 後仅保留 `PATH` + `GIT_PROTOCOL`——環境泄漏在编譯期不可能。

## 推送語义

推送落进 scratch 的本地參照；鏡像波次随後：

1. 把 scratch 的進階參照導入 noa 參照體系（快照/工作區）；
2. `backflow` 開啟时把状態推回 origin。

因此推送不需要任何特殊钩子——三条车道（讀、收、回流）共享同一 scratch，閉環自动
收敛。推送要求已认證身份；无憑證 401 + Basic challenge，reader 角色 403。

## 已知边界

- 推送不提供刪除語义（noa 侧刪除不回傳 origin——已知不对称）。
- dumb HTTP（`/info/refs` 裸文件）不服務。
- PR heads 回流落在 `refs/noa/pr/<N>`（GitHub 拒绝 `refs/pull/*` 推送）。
