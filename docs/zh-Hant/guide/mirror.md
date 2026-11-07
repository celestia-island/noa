# 鏡像與回流

鏡像系统把外部仓（GitHub 等）的 master、全部 tags 與活跃 PR heads 收进 noa；回流车道
再把 noa 的状態推回 origin——构成「noa 權威 + GitHub 鏡像」的过渡形態，farm/CNB 照旧
從 GitHub fetch，零改动。

## 鏡像任務（MirrorJob）

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
| `name` | `^[a-z0-9][a-z0-9-]{0,63}$`；同时是 scratch 目錄名、catalog 仓名、克隆路径段 |
| `remote_url` | 仅允许 `https://` / `file://`；禁止内嵌憑證（`@`、`?`） |
| `github_slug` | `owner/repo` 形状校驗；Git Data API 旁路與回流使用 |
| `branches` | 至少一个；`tags`/`tags/*`、`pr/*`（PR 命名空间衝突）與派生工作區非法形状都被拒绝 |
| `tags` | 鏡像全部 tags |
| `pr_heads` | 鏡像活跃 PR heads（`refs/pull/<N>/head` → `mirror/{job}/pr/<N>`） |
| `backflow` | 每波把 scratch 状態推回 origin（详见下文） |
| `canary_push` | 推一枚探针參照驗證寫方向 |
| `enabled` | 關閉後調度器跳过 |

CRUD：`POST/GET /api/v1/mirror/jobs`、`DELETE /api/v1/mirror/jobs/{name}`、
`POST /api/v1/mirror/run`（手动觸發，body `{"job":"<name>"}`）。

## 雙车道

- **git 车道**：scratch 裸克隆 + `git fetch`——tags、PR heads、回流推送都走它。
- **API 车道**：git 傳輸死亡时（網路抖动族）自动降級为 Git Data API 逐 blob 導入，
  每个 blob 校驗 SHA（忠實度不因车道而降級）。

降級是波次級的：`ensure_scratch` 失败 → 本波走 API 车道 → 自愈刪除半坏 scratch，
下一波重新克隆。回流推送只在 git 车道可用时執行（API 无法推送參照）。

## PR heads

`pr_heads: true` 时每波額外拉取 `refs/pull/<N>/head`，導入为
`mirror/{job}/pr/<N>` + 同名工作區。PR 命名空间與分支命名空间（`pr/<x>` 形状的分支）
互斥，upsert 时拒绝。

## 回流（backflow）

```text
scratch 本地參照 ──atomic push──▶ origin
  +refs/heads/*:refs/heads/*
  +refs/tags/*:refs/tags/*
  +refs/pull-heads/*:refs/noa/pr/*     （仅当 scratch 存在 pull-heads）
```

- **憑證**：`token_env`（預設 `NOA_MIRROR_GH_TOKEN`）解析出的 token 以
  `x-access-token:<token>` 形式嵌入一次性 push URL；token 不落 scratch 設定。
- **force 語义**：翻转後 noa 是寫權威，origin 的參照向 scratch 收敛。刪除不对称
  （noa 侧刪除不会刪除 origin 侧參照）是已知限制。
- **失败語义**：回流失败把波次定为 FAILED——这是驗收關鍵车道，必须显眼。
- **命名空间桥接**：GitHub 拒绝推送 `refs/pull/*`，因此 PR heads 落在
  `refs/noa/pr/<N>`。

## canary 探针

`canary_push: true` 时每波向 origin 推一枚 `noa-mirror-canary` 探针參照（首个已复制
分支的 commit），先走 git 傳輸、失败降級 Git Data API——用於持续驗證寫方向憑證與
連通性。探针失败会让波次響亮失败。

## 忠實度校驗

每个導入的 commit 做逐项对帳：路径集合、条目 kind、blob OID（`git_blob_oid` 與 git
一致）必须完全匹配；任何差異让该项失败并进入波次错誤详情。回流依赖这一不變量——
noa 对象與 git 对象一一对应，推送无需转换層。

## 过期參照清理（reconcile）

tags 與 PR heads 的上游集合是 reconcile 集：不再出现在上游的參照会被清出 noa
（`mirror/{job}/tags/`、`mirror/{job}/pr/` 命名空间）。回流的 tags 推送同样以上游
为准（noa 侧刪除不会刪除 origin 侧——已知的不对称）。

## 調度

`NOA_MIRROR_ENABLED=1` 时調度器每 900 秒跑一輪全部 enabled 任務；`POST /mirror/run`
可手动觸發。波次详情（车道、各项结果、backflow/canary 状態）在
`GET /api/v1/mirror/jobs` 的 `last_detail` / `last_lane`。
