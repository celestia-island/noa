# CLI 参考

`noa` 是工作区侧的命令行入口（clap 派生；`noa --help` 查看实时签名）。所有命令都在
一个已 `noa init` 的仓内运行。

## 命令一览

| 命令 | 用途 |
|---|---|
| `noa init` | 把当前目录初始化为 noa 仓（创建 `.noa` 元数据目录） |
| `noa status` | 当前工作区状态：head、未提交变更、工作区列表 |
| `noa log` | 当前工作区的快照历史（截断渲染 + 父链遍历） |
| `noa snapshot` | 快照操作（创建 / 查看） |
| `noa workspace` | 工作区管理：`workspace create <name>`（按智能体隔离）、切换、列表 |
| `noa remote` | 远端管理（add / set-url / list） |
| `noa push` / `noa pull` / `noa fetch` / `noa clone` | 与远端同步（git 兼容语义） |
| `noa resolve <path>` | 解析嵌套路径到树条目 |
| `noa coauthor` | 为提交附加合作者（智能体协作署名） |
| `noa hook` | 管理仓钩子（带哨兵标记，安全覆盖既有钩子） |
| `noa verify` | 向当前工作区的智能体日志追加 `verify.run/1` 记录（JSON 走 stdin 或 `--file`）——验证账本 CLI 写者 |
| `noa sync` | 同步操作 |
| `noa storage` | 存储后端操作 |
| `noa pr` | PR 生命周期（forge 无关）：`pr create` / `pr list` / `pr diff` 等 |

## 工作区隔离的工作流

```bash
noa init
noa workspace create agent-a      # 智能体 A 的隔离工作区
# … 在工作区内编辑 …
noa snapshot -m "agent-a 的变更"
noa log                           # 只看到本工作区的父链
```

多个智能体各自 `workspace create`，互不可见对方的 head 与内容——这是 noa 与传统
分支工作流的根本差异：隔离单位是工作区，不是引用。

## 服务端协同

CLI 的 push/pull/fetch/clone 与服务端 `/git/*` 智能协议互通（同 git 兼容语义）；对认证远端，
`NOA_GIT_USER` + `NOA_GIT_PASS` 环境变量注入凭据（clone/push/pull 都注入；token 只用于传输、
**永不落盘** `.git/noa/config`）。远端指向 noa-server 时：

```bash
noa remote add origin http://noa.example.com:3016/git/noa
noa push origin master
```

推送走 receive-pack，需要已认证身份（token 即密码；角色需可写）。注意 `noa push` 发布的是
**当前工作区快照**（客户端侧序列化为 `[noa export]` git 提交）——编辑后先 `noa snapshot create`
再 push；直接 push 未快照的改动会发布旧快照。

## verify 与账本

`noa verify` 是验证账本的 CLI 写者：记录以 `verify.run/1` schema 追加到当前工作区的
智能体日志（JSONL，仅追加）。记录可携带 `hardware=<hw-id>` 绑定（配合工作区侧的
硬件注册表审计）。

> 权威账本在文件侧（`/mnt/work/verify-ledger/`）；CLI 写入是 fail-open 双写——开放
> 写者不验链是设计裁决，链校验由读取器完成。
