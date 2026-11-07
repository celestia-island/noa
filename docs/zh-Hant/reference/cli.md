# CLI 参考

`noa` 是工作區侧的命令行入口（clap 派生；`noa --help` 查看實时簽名）。所有命令都在
一个已 `noa init` 的仓内執行。

## 命令一览

| 命令 | 用途 |
|---|---|
| `noa init` | 把当前目錄初始化为 noa 仓（建立 `.noa` 元數據目錄） |
| `noa status` | 当前工作區状態：head、未提交變更、工作區列表 |
| `noa log` | 当前工作區的快照歷史（截斷渲染 + 父鏈遍歷） |
| `noa snapshot` | 快照操作（建立 / 查看） |
| `noa workspace` | 工作區管理：`workspace create <name>`（按智慧代理隔离）、切换、列表 |
| `noa remote` | 遠端管理（add / set-url / list） |
| `noa push` / `noa pull` / `noa fetch` / `noa clone` | 與遠端同步（git 兼容語义） |
| `noa resolve <path>` | 解析嵌套路径到树条目 |
| `noa coauthor` | 为提交附加合作者（智慧代理协作署名） |
| `noa hook` | 管理仓钩子（带哨兵標記，安全覆盖既有钩子） |
| `noa verify` | 向当前工作區的智慧代理日誌追加 `verify.run/1` 記錄（JSON 走 stdin 或 `--file`）——驗證帳本 CLI 寫者 |
| `noa sync` | 同步操作 |
| `noa storage` | 存储後端操作 |
| `noa pr` | PR 生命周期（forge 无關）：`pr create` / `pr list` / `pr diff` 等 |

## 工作區隔离的工作流

```bash
noa init
noa workspace create agent-a      # 智慧代理 A 的隔离工作區
# … 在工作區内编辑 …
noa snapshot -m "agent-a 的變更"
noa log                           # 只看到本工作區的父鏈
```

多个智慧代理各自 `workspace create`，互不可见对方的 head 與内容——这是 noa 與傳统
分支工作流的根本差異：隔离單位是工作區，不是參照。

## 伺服器端协同

CLI 的 push/pull/fetch/clone 與伺服器端 `/git/*` 智能协議互通（同 git 兼容語义）。遠端
指向 noa-server 时：

```bash
noa remote add origin http://noa.example.com:3016/git/noa
noa push origin master
```

推送走 receive-pack，需要已认證身份（token 即密碼；角色需可寫）。

## verify 與帳本

`noa verify` 是驗證帳本的 CLI 寫者：記錄以 `verify.run/1` schema 追加到当前工作區的
智慧代理日誌（JSONL，仅追加）。記錄可携带 `hardware=<hw-id>` 绑定（配合工作區侧的
硬件注册表审计）。

> 權威帳本在文件侧（`/mnt/work/verify-ledger/`）；CLI 寫入是 fail-open 雙寫——開放
> 寫者不驗鏈是設计裁决，鏈校驗由讀取器完成。
