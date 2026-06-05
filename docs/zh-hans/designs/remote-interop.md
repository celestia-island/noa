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

```mermaid
graph LR
    subgraph Noa
        NB["noa blob：<br/>原始字节<br/>SHA-256 哈希"]
    end
    subgraph Git
        GB["Git blob：<br/>'blob &lt;size&gt;\\0&lt;content&gt;'<br/>SHA-1 哈希"]
    end
    NB -- "使用 Git 的 blob 头部格式<br/>重新哈希内容" --> GB
```

### Tree ↔ Git Tree

```mermaid
graph LR
    subgraph Noa
        NT["noa tree：<br/>MessagePack [{name, kind, hash}]"]
    end
    subgraph Git
        GT["Git tree：<br/>'&lt;mode&gt; &lt;name&gt;\\0&lt;20 字节 sha1&gt;' 条目"]
    end
    NT -- "TreeEntry::Blob → mode 100644<br/>TreeEntry::Tree → mode 040000<br/>SHA-256 → SHA-1 重新哈希" --> GT
```

### Snapshot ↔ Git Commit

```mermaid
graph LR
    subgraph "noa 快照"
        NS["id: noa_abc123<br/>tree_hash: SHA-256<br/>parents: [noa_...]<br/>author: agent-001<br/>timestamp: 1717592400000000（µs）<br/>message: '添加功能'"]
    end
    subgraph "Git 提交"
        GC["tree: SHA-1<br/>parent: SHA-1<br/>author: agent-001 &lt;agent@noa&gt;<br/>message: '添加功能'"]
    end
    NS -- "tree_hash 重新哈希（SHA-256 → SHA-1）<br/>通过 ID 查找表映射 parent<br/>使用虚拟邮箱格式化 author<br/>µs 时间戳截断为秒" --> GC
```

### Workspace ↔ Git Branch

```mermaid
graph LR
    subgraph Noa
        NW["工作区 'feature-1'<br/>(head：noa_abc123)"]
        ND["工作区 'default'<br/>(head：noa_def456)"]
    end
    subgraph Git
        GB1["分支 'feature-1'<br/>(HEAD：git-sha1)"]
        GB2["分支 'main'<br/>(HEAD：git-sha1)"]
    end
    NW --> GB1
    ND --> GB2
```

## MinIO/S3 后端

对于无需 Git 基础设施的部署：

```mermaid
flowchart TD
    A["noa push --remote s3-remote"] --> B["PUT /bucket/snapshots/noa_abc123（msgpack）"]
    A --> C["PUT /bucket/blobs/&lt;sha256&gt;（原始字节）"]
    A --> D["PUT /bucket/trees/&lt;sha256&gt;（msgpack）"]
    A --> E["PUT /bucket/refs/default（快照 ID 文本）"]
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
