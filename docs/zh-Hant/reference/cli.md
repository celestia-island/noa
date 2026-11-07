# CLI 參考

`noa` 是工作區側的命令列入口（clap 派生；`noa --help` 查看即時簽名）。所有命令都在
一個已 `noa init` 的儲存庫內執行。

## 命令一覽

| 命令 | 用途 |
|---|---|
| `noa init` | 把目前目錄初始化為 noa 儲存庫（建立 `.noa` 中繼資料目錄） |
| `noa status` | 目前工作區狀態：head、未提交的變更、工作區列表 |
| `noa log` | 目前工作區的快照歷史（截斷渲染 + 父鏈走訪） |
| `noa snapshot` | 快照操作（建立 / 查看） |
| `noa workspace` | 工作區管理：`workspace create <name>`（按智慧型代理隔離）、切換、列表 |
| `noa remote` | 遠端管理（add / set-url / list） |
| `noa push` / `noa pull` / `noa fetch` / `noa clone` | 與遠端同步（git 相容語意） |
| `noa resolve <path>` | 解析巢狀路徑到樹條目 |
| `noa coauthor` | 為提交附加合作者（智慧型代理協作署名） |
| `noa hook` | 管理儲存庫鉤子（帶哨兵標記，可安全覆蓋既有鉤子） |
| `noa verify` | 向目前工作區的智慧型代理日誌追加 `verify.run/1` 紀錄（JSON 走 stdin 或 `--file`）——驗證帳本的 CLI 寫入者 |
| `noa sync` | 同步操作 |
| `noa storage` | 儲存後端操作 |
| `noa pr` | PR 生命週期（forge 無關）：`pr create` / `pr list` / `pr diff` 等 |

## 工作區隔離的工作流程

```bash
noa init
noa workspace create agent-a      # 智慧型代理 A 的隔離工作區
# … 在工作區內編輯 …
noa snapshot -m "agent-a 的變更"
noa log                           # 只看得到本工作區的父鏈
```

多個智慧型代理各自 `workspace create`，互不可見對方的 head 與內容——這是 noa 與傳統
分支工作流程的根本差異：隔離單位是工作區，不是引用。

## 伺服器端協同

CLI 的 push/pull/fetch/clone 與伺服器端 `/git/*` 智慧協定互通（同 git 相容語意）。遠端
指向 noa-server 時：

```bash
noa remote add origin http://noa.example.com:3016/git/noa
noa push origin master
```

推送走 receive-pack，需要已驗證身分（token 即密碼；角色需可寫）。

## verify 與帳本

`noa verify` 是驗證帳本的 CLI 寫入者：紀錄以 `verify.run/1` schema 附加到目前工作區的
智慧型代理日誌（JSONL，僅追加）。紀錄可攜帶 `hardware=<hw-id>` 綁定（配合工作區側的
硬體註冊表稽核）。

> 權威帳本在檔案側（`/mnt/work/verify-ledger/`）；CLI 寫入是 fail-open 雙寫——開放
> 寫入者不驗鏈是設計裁決，鏈校驗由讀取器完成。
