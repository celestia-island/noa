# 镜像与回流

镜像系统把外部仓（GitHub 等）的 master、全部 tags 与活跃 PR heads 收进 noa；回流车道
再把 noa 的状态推回 origin——构成「noa 权威 + GitHub 镜像」的过渡形态，farm/CNB 照旧
从 GitHub fetch，零改动。

## 镜像任务（MirrorJob）

```json
{
  "name": "noa",
  "remote_url": "https://github.com/celestia-island/noa.git",
  "github_slug": "celestia-island/noa",
  "branches": ["master"],
  "tags": true,
  "pr_heads": true,
  "backflow": true,
  "canary_push": true,
  "enabled": true
}
```

| 字段 | 说明 |
|---|---|
| `name` | `^[a-z0-9][a-z0-9-]{0,63}$`；同时是 scratch 目录名、catalog 仓名、克隆路径段 |
| `remote_url` | 仅允许 `https://` / `file://`；禁止内嵌凭据（`@`、`?`） |
| `github_slug` | `owner/repo` 形状校验；Git Data API 旁路与回流使用 |
| `branches` | 至少一个；`tags`/`tags/*`、`pr/*`（PR 命名空间冲突）与派生工作区非法形状都被拒绝 |
| `tags` | 镜像全部 tags |
| `pr_heads` | 镜像活跃 PR heads（`refs/pull/<N>/head` → `mirror/{job}/pr/<N>`） |
| `backflow` | 每波把 scratch 状态推回 origin（详见下文） |
| `canary_push` | 推一枚探针引用验证写方向 |
| `enabled` | 关闭后调度器跳过 |

CRUD：`POST/GET /api/v1/mirror/jobs`、`DELETE /api/v1/mirror/jobs/{name}`、
`POST /api/v1/mirror/run`（手动触发，body `{"job":"<name>"}`）。

## 双车道

- **git 车道**：scratch 裸克隆 + `git fetch`——tags、PR heads、回流推送都走它。
- **API 车道**：git 传输死亡时（网络抖动族）自动降级为 Git Data API 逐 blob 导入，
  每个 blob 校验 SHA（忠实度不因车道而降级）。

降级是波次级的：`ensure_scratch` 失败 → 本波走 API 车道 → 自愈删除半坏 scratch，
下一波重新克隆。回流推送只在 git 车道可用时执行（API 无法推送引用）。

## PR heads

`pr_heads: true` 时每波额外拉取 `refs/pull/<N>/head`，导入为
`mirror/{job}/pr/<N>` + 同名工作区。PR 命名空间与分支命名空间（`pr/<x>` 形状的分支）
互斥，upsert 时拒绝。

## 回流（backflow）

```text
scratch 本地引用 ──atomic push──▶ origin
  +refs/heads/*:refs/heads/*
  +refs/tags/*:refs/tags/*
  +refs/pull-heads/*:refs/noa/pr/*     （仅当 scratch 存在 pull-heads）
```

- **凭据**：`token_env`（默认 `NOA_MIRROR_GH_TOKEN`）解析出的 token 以
  `x-access-token:<token>` 形式嵌入一次性 push URL；token 不落 scratch 配置。
- **force 语义**：翻转后 noa 是写权威，origin 的引用向 scratch 收敛。删除不对称
  （noa 侧删除不会删除 origin 侧引用）是已知限制。
- **失败语义**：回流失败把波次置为 FAILED——这是验收关键车道，必须显眼。
- **命名空间桥接**：GitHub 拒绝推送 `refs/pull/*`，因此 PR heads 落在
  `refs/noa/pr/<N>`。

## canary 探针

`canary_push: true` 时每波向 origin 推一枚 `noa-mirror-canary` 探针引用（首个已复制
分支的 commit），先走 git 传输、失败降级 Git Data API——用于持续验证写方向凭据与
连通性。探针失败会让波次响亮失败。

## 忠实度校验

每个导入的 commit 做逐项对账：路径集合、条目 kind、blob OID（`git_blob_oid` 与 git
一致）必须完全匹配；任何差异让该项失败并进入波次错误详情。回流依赖这一不变量——
noa 对象与 git 对象一一对应，推送无需转换层。

## 过期引用清理（reconcile）

tags 与 PR heads 的上游集合是 reconcile 集：不再出现在上游的引用会被清出 noa
（`mirror/{job}/tags/`、`mirror/{job}/pr/` 命名空间）。回流的 tags 推送同样以上游
为准（noa 侧删除不会删除 origin 侧——已知的不对称）。

## 调度

`NOA_MIRROR_ENABLED=1` 时调度器每 900 秒跑一轮全部 enabled 任务；`POST /mirror/run`
可手动触发。波次详情（车道、各项结果、backflow/canary 状态）在
`GET /api/v1/mirror/jobs` 的 `last_detail` / `last_lane`。
