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

### Ref 映射

```mermaid
graph LR
    subgraph "noa Refs"
        NH["HEAD → default"]
        ND2["default → noa_abc"]
        NF["feature-1 → noa_def"]
    end
    subgraph "Git Refs"
        GH["HEAD → refs/heads/main"]
        GMAIN["refs/heads/main → git-sha1"]
        GF1["refs/heads/feature-1 → git-sha2"]
    end
    NH -.-> GH
    ND2 -.-> GMAIN
    NF -.-> GF1
```

## 推送流程

```mermaid
flowchart TD
    A["1. noa push --remote origin"] --> B["2. 加载从工作区 head 可到达的所有快照"]
    B --> C["3. 将每个快照转换为 Git commit"]
    C --> D["4. 将每个 blob/tree 转换为 Git object"]
    D --> E["5. 通过 gix (gitoxide) 推送到 origin URL"]
    E --> F["6. 更新远程 refs"]
```

## 拉取流程

```mermaid
flowchart TD
    A["1. noa pull --remote origin"] --> B["2. 通过 gix 获取 refs"]
    B --> C["3. 对每个新的 Git commit："]
    C --> D["a. 转换为 noa 快照<br/>b. 将 blobs/trees 转换为 noa 对象<br/>c. 存储在本地 redb 中"]
    D --> E["4. 创建合并快照（本地 head + 远程 head）"]
    E --> F["5. 更新工作区 head"]
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

## 远程配置

存储在 `.noa/config`（TOML）中：

```toml
[[remotes]]
name = "origin"
url = "https://github.com/example/repo.git"
backend = "git"

[[remotes]]
name = "s3"
url = "s3://my-bucket/noa-repo"
backend = "minio"
endpoint = "https://s3.amazonaws.com"
region = "us-east-1"
```

## 远程互操作方案对比

| 方案 | 使用者 | 优势 | 缺点 |
|------|--------|------|------|
| Git 桥接 (gix) | noa | 通用兼容性 | 转换开销，SHA-1/SHA-256 不匹配 |
| 原生协议 | Git | 快速，无需转换 | 仅适用 Git |
| WebDAV | SVN | HTTP 标准 | 有限，SVN 特定 |
| REST API | Bitbucket | 现代，灵活 | 需要托管服务 |
| S3 兼容存储 | noa | 可扩展，云原生 | 无桥接时无法与 Git 互操作 |

noa 同时支持 Git 桥接（用于兼容性）和原生 S3（用于规模）。
