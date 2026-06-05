# 远程互操作设计

## 概述

noa 支持多种远程后端，用于跨机器和团队同步快照和对象。主要的互操作目标是 Git，支持与 GitHub、GitLab 和 Bitbucket 现有工作流的无缝集成。

## 远程后端 Trait

```rust
#[async_trait]
pub trait RemoteBackend: Send + Sync {
    async fn push_snapshots(&self, ids: &[SnapshotId]) -> Result<()>;
    async fn fetch_snapshots(&self, ids: &[SnapshotId]) -> Result<Vec<Snapshot>>;
    async fn push_objects(&self, ids: &[String]) -> Result<()>;
    async fn fetch_objects(&self, ids: &[String]) -> Result<()>;
    async fn list_refs(&self) -> Result<HashMap<String, SnapshotId>>;
    async fn update_ref(&self, name: &str, old: Option<&SnapshotId>, new: &SnapshotId) -> Result<()>;
}
```

## Git 转换层

`GitTranslator` 在 noa 的对象模型和 Git 之间转换：

### Blob ↔ Git Blob

```
noa blob：  原始字节，SHA-256 哈希
Git blob：  "blob <size>\0<content>"，SHA-1 哈希

转换：使用 Git 的 blob 头部格式重新哈希内容
```

### Tree ↔ Git Tree

```
noa tree：  MessagePack [{name, kind, hash}]
Git tree：  "<mode> <name>\0<20 字节 sha1>" 条目

转换：
  noa TreeEntry::Blob  → Git mode 100644
  noa TreeEntry::Tree  → Git mode 040000
  需要 SHA-256 → SHA-1 重新哈希
```

### Snapshot ↔ Git Commit

```
noa 快照：
  id:        noa_abc123
  tree_hash: SHA-256
  parents:   [noa_...]
  author:    agent-001
  timestamp: 1717592400000000（µs）
  message:   "添加功能"

Git 提交：
  tree:      SHA-1
  parent:    SHA-1
  author:    agent-001 <agent@noa> <unix-timestamp> <tz>
  message:   "添加功能"

转换：
  - tree_hash 重新哈希（SHA-256 → SHA-1）
  - 通过 ID 查找表映射 parent
  - 使用虚拟邮箱格式化 author
  - µs 时间戳截断为秒
```

### Workspace ↔ Git Branch

```
noa 工作区 "feature-1"（head：noa_abc123）
  → Git 分支 "feature-1"（HEAD：git-sha1）

noa 工作区 "default"（head：noa_def456）
  → Git 分支 "main"（HEAD：git-sha1）
```

## MinIO/S3 后端

对于无需 Git 基础设施的部署：

```
noa push --remote s3-remote
  → PUT /bucket/snapshots/noa_abc123（msgpack）
  → PUT /bucket/blobs/<sha256>（原始字节）
  → PUT /bucket/trees/<sha256>（msgpack）
  → PUT /bucket/refs/default（快照 ID 文本）
```

优势：
- 无需 tree/快照转换（原生 noa 格式）
- 直接 blob 存储（无 Pack 文件开销）
- S3 兼容（适用于 AWS、GCS、MinIO、Cloudflare R2）

## 认证

| 后端 | 方法 |
|------|------|
| Git HTTPS | 从 `~/.git-credentials` 获取凭证或提示输入 |
| Git SSH | SSH agent 或密钥文件 |
| MinIO/S3 | 访问密钥 + 秘密密钥（环境变量或配置） |
