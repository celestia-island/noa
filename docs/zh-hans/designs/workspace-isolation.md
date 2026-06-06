# 工作区隔离设计

## 概述

工作区为代理和人类提供隔离的工作上下文。每个工作区都有独立的状态（head 快照、代理日志），同时共享底层对象存储。

## 工作区结构

```rust
pub struct Workspace {
    pub name: String,
    pub head: SnapshotId,          // 当前快照
    pub base: SnapshotId,          // 从父工作区分叉点
    pub agent_id: Option<String>,  // 关联的代理
    pub created_at: u64,
    pub updated_at: u64,
}
```

## 工作区生命周期

```mermaid
flowchart LR
    A["创建"] --> B["切换"]
    B --> C["（代理写入 + 快照）"]
    C --> D["合并"]
    D --> E["删除"]
```

### 创建

```bash
noa workspace create feature-1
```

1. 读取当前工作区的 head 快照 → 成为 `base`
2. 新工作区：`head = base`（继承当前状态）
3. 创建代理日志文件：`agent-logs/feature-1.log`
4. 在 WorkspaceStore 中注册

### 切换

```bash
noa workspace switch feature-1
```

1. 验证工作区存在
2. 将工作区名称写入 `.noa/HEAD`
3. 将之前的工作区保存到 `.noa/ORIG_HEAD`

### 合并

```bash
noa workspace merge feature-1
```

1. 三路合并：base → ours（当前） vs theirs（feature-1）
2. 创建合并快照，双方均为 parent
3. 更新当前工作区 head

### 删除

```bash
noa workspace delete feature-1
```

1. 验证不是活动工作区
2. 从存储中删除工作区条目
3. 删除代理日志文件
4. 对象保留（共享，内容寻址）

## HEAD 文件

`.noa/HEAD` 包含活动工作区名称：

```
feature-1
```

`.noa/ORIG_HEAD` 包含上一个工作区（用于撤销）：

```
default
```

## 工作区存储

工作区存储在 redb 中：

```
Table：workspaces
  Key："feature-1"（工作区名称，&str 类型）
  Value：msgpack(Workspace)，&[u8] 类型
```

Head 更新使用 CAS（比较并交换）：

```rust
async fn update_head(&self, name: &str, expected: &SnapshotId, new: &SnapshotId) -> Result<()>
```

这防止了多个进程尝试同时更新同一工作区时的丢失更新。

## 与 Git 分支的对比

| 方面 | noa 工作区 | Git 分支 |
|------|-----------|----------|
| 存储 | redb 表条目 | ref 文件（`.git/refs/heads/`） |
| 隔离 | 自有代理日志文件 | 共享 index + 工作树 |
| 切换 | 原子 HEAD 写入 | 工作树检出新文件（文件 I/O） |
| 创建 | O(1) — 仅元数据 | O(1) — 轻量 |
| 删除 | 从存储中删除 | 删除 ref，可选 prune |
| 代理绑定 | 可选 agent_id 字段 | 无等效项 |
| Base 跟踪 | 显式 base 字段 | 隐式（合并基础） |

## 与 SVN 分支的对比

| 方面 | noa 工作区 | SVN 分支 |
|------|-----------|----------|
| 存储 | KV 条目 | 完整目录副本 |
| 创建 | O(1) 元数据 | O(n) 文件副本 |
| 隔离 | 逻辑（共享对象） | 物理（独立目录） |
| 合并跟踪 | 父节点 DAG | svn:mergeinfo 属性 |

## 设计理由

### 为什么用工作区而非分支？

1. **代理身份**：工作区携带 `agent_id` 字段用于归属
2. **代理日志隔离**：每个工作区有专属日志文件
3. **无工作树**：noa 不维护检出版本——只有快照
4. **显式 base**：`base` 字段支持快速合并基础计算

### 为什么没有工作树检出版本？

Git 分支需要工作树检出版本（每个切换的文件都需要文件 I/O）。
noa 工作区只切换指针——代理日志和快照引用。无论仓库大小，这都是 O(1) 操作。

文件物化（检出）在代理需要读取或写入实际文件时单独发生，以快照的 tree 为真实来源。
