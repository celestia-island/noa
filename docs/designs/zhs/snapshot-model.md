# 快照模型设计

## 概述

快照是工作区完整文件树状态在某一时间点的不可变、内容寻址的记录。快照通过父引用形成有向无环图（DAG）。

## 快照结构

```rust
pub struct SnapshotId(pub String);  // "noa_<12-字符-base62>"

pub struct Snapshot {
    pub id: SnapshotId,
    pub tree_hash: String,           // 根树的 SHA-256
    pub parents: Vec<SnapshotId>,    // 0-N 个父快照
    pub workspace: String,           // 来源工作区
    pub author: String,              // 代理或人工标识符
    pub timestamp: u64,              // Unix 纪元后的微秒数
    pub message: String,             // 人类可读的描述
}
```

## ID 生成

快照 ID 使用 12 字符的 base62 字符串，前缀为 `noa_`：

```
noa_3kF8x2mP9aB1
```

生成方式：`SHA256(tree_hash || parents || workspace || timestamp)[0..9]` 以 base62 编码。这提供：
- 62^12 ≈ 3.2 × 10^21 可能的 ID
- 碰撞概率实际为零
- 确定性：相同输入 → 相同 ID（支持去重）

## 快照 DAG

```
noa_empty（哨兵）
    │
    ├── noa_abc123（工作区: default, "初始化"）
    │       │
    │       ├── noa_def456（工作区: feature-1, "添加登录"）
    │       │       │
    │       │       └── noa_ghi789（工作区: feature-1, "添加测试"）
    │       │
    │       └── noa_jkl012（工作区: feature-2, "修复 bug"）
    │
    └── noa_mno345（合并 feature-1 和 feature-2 到 default）
            parents: [noa_abc123, noa_ghi789, noa_jkl012]
```

## 快照创建流程

```
1. AgentLog 重放
   └── 读取工作区所有 write/delete/rename 操作

2. Tree 构建
   └── 从父快照的 tree 开始
   └── 按顺序应用操作
   └── 存储结果 tree → ObjectStore

3. 快照创建
   └── 构建 Snapshot 结构，包含 tree_hash
   └── 从内容计算 ID
   └── 存储到 SnapshotStore（redb 表）

4. 工作区更新
   └── CAS 更新工作区 head 到新快照 ID
```

## 差异算法

`diff_snapshots(base, other)` 产生文件级别的变更列表：

```rust
pub struct FileDiff {
    pub path: String,
    pub kind: DiffKind, // Added, Removed, Modified
    pub old_blob: Option<String>,
    pub new_blob: Option<String>,
}
```

算法：
1. 加载两个快照的根 tree
2. 同时递归遍历两个 tree
3. 比较每个路径的 blob 哈希
4. 不同哈希 → Modified；仅一侧存在 → Added/Removed

时间复杂度：O(n)，其中 n = 两个 tree 中的文件总数。

## 哨兵快照

`noa_empty` 是保留的快照 ID，代表空 tree。所有新仓库以此为基础。它不会被显式存储——工作区管理器将其识别为"暂无快照"。

## 与 Git 提交的对比

| 方面 | noa 快照 | Git 提交 |
|------|----------|----------|
| ID 格式 | `noa_<base62>` | SHA-1 十六进制 |
| 父限制 | 无限制（合并 DAG） | 通常 1-2 个 |
| Tree 格式 | MessagePack | 自定义二进制 |
| 时间戳 | 微秒精度 | 秒精度 + 时区 |
| 作者字段 | 代理 ID 或人工 | 姓名 + 邮箱 |
| 不可变性 | 存储层强制 | 哈希强制 |
| GPG 签名 | 不支持 | 支持 |
