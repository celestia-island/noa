# 架构总览

noa 是面向智能体的原生分布式版本控制系统。它不是「git 的包装」：核心对象模型自带
SHA-256 内容寻址，同时与 git 的对象代数保持逐字节兼容（同一文件内容在 noa 与 git 中
算出相同的 blob OID——镜像忠实度校验正依赖这一点）。

## 三层模型

```
工作区（Workspace）   —— 每个智能体一个隔离工作区，互不可见
   │ head 指向
快照（Snapshot）      —— 内容寻址的提交（SHA-256），DAG 结构（可多父 = 合并）
   │ tree_hash 指向
对象（ObjectStore）   —— blob / tree / symlink / gitlink（blob 内容与 git 逐字节兼容；树/提交用 noa 自身的 SHA-256 寻址）
```

- **工作区**是 noa 的核心隔离原语：多个智能体在同一仓内各自持有独立的工作区与 head，
  互不覆盖。`mirror.<job>.<branch>` 这类系统命名空间同样以工作区形式存在。
- **快照**是提交的等价物：`tree_hash` + `parents[]` + `author` + `timestamp` + `message`。
  快照 ID 形如 `noa_<sha256>`（内容寻址；相同树哈希的重复导入幂等）。
- **引用**（RedbRefStore）：`HEAD`、`mirror/{job}/{branch}`、`mirror/{job}/tags/{tag}`、
  `mirror/{job}/pr/{N}` 等全部是 redb 里的引用行，指向快照 ID。

存储引擎是单文件嵌入式 KV（redb）——部署即拷贝目录，无需外部数据库。

## 服务端组件（noa-server）

| 组件 | 职责 |
|---|---|
| REST API（`/api/v1/*`） | repos / PRs / runs / users / mirror-jobs 的 CRUD |
| git-http（`/git/{repo}/*`） | smart HTTP：upload-pack（clone/fetch）+ receive-pack（push，W2a2 起） |
| 镜像引擎 | 定时波次：从 origin 拉取 master / tags / PR heads 导入 noa；可回流推送 |
| run-records | CI 运行记录存储（chest DevOps 面板的数据源） |
| 鉴权中间件 | 路由级 bearer 认证 + 角色执法（REST）与 can_write 执法（git push） |

所有服务端状态都在一个 redb 数据库里；服务端无状态化到「目录即全部」。

## git 兼容性的精确边界

兼容性分三层，逐层说清：

- **blob：逐字节兼容。** 原始内容即 git blob 体，`git_blob_oid(content)` 与 git 一致
  （镜像忠实度校验逐 blob 对账就靠这个）；
- **树：语义等价，ID 不等价。** 路径/kind/每个 blob 的 OID 与 git 完全对应，但 noa 的
  tree ID 是自己的 SHA-256 内容寻址，不是 git 的树对象格式；
- **提交：noa 自己的模型。** 快照 ID 形如 `noa_<sha256>`，不是 git commit 对象。

git 形态的表示存在于**镜像 scratch 的真 git 仓**里。回流推送直接使用 scratch 的
git 对象（对「从 git 导入的内容」零转换）；**noa 原生产生的内容**（纯工作区快照，
从未经过 scratch）没有 git 对象形态，直接回流需要快照 → git 对象序列化器——这是
已知限制（现有实现中此类引用不会出现在回流推送里）。

## 镜像 scratch 的角色

`NOA_MIRROR_SCRATCH_DIR/{job}.git` 是每个镜像任务的裸克隆，承担三重角色：

1. **拉取源缓存**——波次从 origin fetch，导入器从这里读 git 对象；
2. **git-http 的服务体**——`/git/{repo}` 直接对其跑 upload-pack / receive-pack；
3. **回流的推送源**——backflow 车道把它的本地引用原子推回 origin。

推送（receive-pack）更新 scratch 的本地引用；下一个波次把它导入 noa 引用体系，再由
backflow 车道推回 origin——三条路径共享同一份 scratch，闭环即成。

## 目录索引

- [快速上手](./getting-started.md)
- [镜像与回流](./mirror.md)
- [权限与用户](./rbac.md)
- [git-http 协议](./git-http.md)
- [合并与冲突](./merge.md)
- [CI 运行记录](./ci-runs.md)
- [HTTP API 参考](../reference/http-api.md)
- [CLI 参考](../reference/cli.md)
- [部署](../reference/deploy.md)
