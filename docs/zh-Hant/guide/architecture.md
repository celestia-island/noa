# 架構總览

noa 是面向智慧代理的原生分布式版本控制系统。它不是「git 的包装」：核心对象模型自带
SHA-256 內容定址，同时與 git 的对象代數保持逐字節兼容（同一文件内容在 noa 與 git 中
算出相同的 blob OID——鏡像忠實度校驗正依赖这一點）。

## 三層模型

```
工作區（Workspace）   —— 每个智慧代理一个隔离工作區，互不可见
   │ head 指向
快照（Snapshot）      —— 內容定址的提交（SHA-256），DAG 结构（可多父 = 合併）
   │ tree_hash 指向
对象（ObjectStore）   —— blob / tree / symlink / gitlink，git 对象代數兼容
```

- **工作區**是 noa 的核心隔离原語：多个智慧代理在同一仓内各自持有独立的工作區與 head，
  互不覆盖。`mirror.<job>.<branch>` 这類系统命名空间同样以工作區形式存在。
- **快照**是提交的等價物：`tree_hash` + `parents[]` + `author` + `timestamp` + `message`。
  快照 ID 形如 `noa_<sha256>`（內容定址；相同树哈希的重复導入幂等）。
- **參照**（RedbRefStore）：`HEAD`、`mirror/{job}/{branch}`、`mirror/{job}/tags/{tag}`、
  `mirror/{job}/pr/{N}` 等全部是 redb 里的參照行，指向快照 ID。

存储引擎是單文件嵌入式 KV（redb）——部署即拷贝目錄，无需外部資料庫。

## 伺服器端組件（noa-server）

| 組件 | 职责 |
|---|---|
| REST API（`/api/v1/*`） | repos / PRs / runs / users / mirror-jobs 的 CRUD |
| git-http（`/git/{repo}/*`） | smart HTTP：upload-pack（clone/fetch）+ receive-pack（push，W2a2 起） |
| 鏡像引擎 | 定时波次：從 origin 拉取 master / tags / PR heads 導入 noa；可回流推送 |
| run-records | CI 執行紀錄存储（chest DevOps 面板的數據源） |
| 鑑權中间件 | 路由級 bearer 认證 + 角色執法（REST）與 can_write 執法（git push） |

所有伺服器端状態都在一个 redb 資料庫里；伺服器端无状態化到「目錄即全部」。

## git 对象兼容性

導入與回流都以「git 对象代數不變」为前提：

- blob：原始内容即 git blob 體（`git_blob_oid(content)` 與 git 一致）；
- 树：導入按路径/kind/oid 逐项对帳（fidelity 校驗）；
- 提交：父鏈由鏡像波次维护（`noa_<sha>` 內容定址 + parent 關联）。

因此 noa → git 的回流推送不需要任何转换層：scratch 克隆里的 git 对象與 noa 的对象
一一对应。

## 鏡像 scratch 的角色

`NOA_MIRROR_SCRATCH_DIR/{job}.git` 是每个鏡像任務的裸克隆，承担三重角色：

1. **拉取源緩存**——波次從 origin fetch，導入器從这里讀 git 对象；
2. **git-http 的服務體**——`/git/{repo}` 直接对其跑 upload-pack / receive-pack；
3. **回流的推送源**——backflow 车道把它的本地參照原子推回 origin。

推送（receive-pack）更新 scratch 的本地參照；下一个波次把它導入 noa 參照體系，再由
backflow 车道推回 origin——三条路径共享同一份 scratch，閉環即成。

## 目錄索引

- [快速上手](./getting-started.md)
- [鏡像與回流](./mirror.md)
- [權限與用戶](./rbac.md)
- [git-http 协議](./git-http.md)
- [合併與衝突](./merge.md)
- [CI 執行紀錄](./ci-runs.md)
- [HTTP API 参考](../reference/http-api.md)
- [CLI 参考](../reference/cli.md)
- [部署](../reference/deploy.md)
