# 架構總覽

noa 是為智慧型代理打造的原生分散式版本控制系統。它不是「git 的包裝」：核心物件模型自帶
SHA-256 內容定址，同時與 git 的物件代數保持逐位元組相容（同一檔案內容在 noa 與 git 中
算出相同的 blob OID——鏡像忠實度校驗正依賴這一點）。

## 三層模型

```
工作區（Workspace）   —— 每個智慧型代理一個隔離工作區，互不可見
   │ head 指向
快照（Snapshot）      —— 內容定址的提交（SHA-256），DAG 結構（可多父 = 合併）
   │ tree_hash 指向
物件（ObjectStore）   —— blob / tree / symlink / gitlink，git 物件代數相容
```

- **工作區**是 noa 的核心隔離原語：多個智慧型代理在同一倉內各自持有獨立的工作區與 head，
  互不覆蓋。`mirror.<job>.<branch>` 這類系統命名空間同樣以工作區形式存在。
- **快照**是提交的等價物：`tree_hash` + `parents[]` + `author` + `timestamp` + `message`。
  快照 ID 形如 `noa_<sha256>`（內容定址；相同樹雜湊的重複匯入具有冪等性）。
- **引用**（RedbRefStore）：`HEAD`、`mirror/{job}/{branch}`、`mirror/{job}/tags/{tag}`、
  `mirror/{job}/pr/{N}` 等全部是 redb 裡的引用條目，指向快照 ID。

儲存引擎是單檔內嵌式 KV（redb）——部署即複製目錄，無需外部資料庫。

## 伺服器端元件（noa-server）

| 元件 | 職責 |
|---|---|
| REST API（`/api/v1/*`） | repos / PRs / runs / users / mirror-jobs 的 CRUD |
| git-http（`/git/{repo}/*`） | smart HTTP：upload-pack（clone/fetch）+ receive-pack（push，自 W2a2 起） |
| 鏡像引擎 | 定時波次：從 origin 擷取 master / tags / PR heads 匯入 noa；可回流推送 |
| run-records | CI 執行紀錄儲存（chest DevOps 面板的資料來源） |
| 驗證中介層 | 路由層級 bearer 驗證 + 角色強制執行（REST）與 can_write 強制執行（git push） |

所有伺服器端狀態都在同一個 redb 資料庫裡；伺服器端無狀態化到「目錄即全部」的程度。

## git 物件相容性

匯入與回流都以「git 物件代數不變」為前提：

- blob：原始內容即 git blob 本體（`git_blob_oid(content)` 與 git 一致）；
- 樹：匯入時按路徑/kind/oid 逐項對帳（fidelity 校驗）；
- 提交：父鏈由鏡像波次維護（`noa_<sha>` 內容定址 + parent 關聯）。

因此 noa → git 的回流推送不需要任何轉換層：scratch 裸儲存庫裡的 git 物件與 noa 的物件
一一對應。

## 鏡像 scratch 的角色

`NOA_MIRROR_SCRATCH_DIR/{job}.git` 是每個鏡像任務的裸儲存庫，承擔三重角色：

1. **擷取來源快取**——波次從 origin fetch，匯入器從這裡讀取 git 物件；
2. **git-http 的服務主體**——`/git/{repo}` 直接對它執行 upload-pack / receive-pack；
3. **回流的推送來源**——backflow 車道把它的本地引用原子推送回 origin。

推送（receive-pack）更新 scratch 的本地引用；下一個波次把它匯入 noa 引用體系，再由
backflow 車道推回 origin——三條路徑共享同一份 scratch，閉迴路就此成形。

## 目錄索引

- [快速上手](./getting-started.md)
- [鏡像與回流](./mirror.md)
- [權限與使用者](./rbac.md)
- [git-http 協定](./git-http.md)
- [合併與衝突](./merge.md)
- [CI 執行紀錄](./ci-runs.md)
- [HTTP API 參考](../reference/http-api.md)
- [CLI 參考](../reference/cli.md)
- [部署](../reference/deploy.md)
