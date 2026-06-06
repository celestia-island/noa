# 遠端互通性設計

## 概述

noa 支援多種遠端後端，用於跨機器和團隊同步快照與物件。主要的互通性目標是 Git，可實現與 GitHub、GitLab 和 Bitbucket 上現有工作流程的無縫整合。

## 遠端後端 Trait

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

## Git 轉譯層

`GitTranslator` 在 noa 的物件模型和 Git 的物件模型之間進行轉換：

### Blob ↔ Git Blob

```mermaid
graph LR
    subgraph Noa
        NB["noa blob：<br/>原始位元組<br/>SHA-256 雜湊"]
    end
    subgraph Git
        GB["Git blob：<br/>'blob &lt;size&gt;\\0&lt;content&gt;'<br/>SHA-1 雜湊"]
    end
    NB -- "使用 Git 的 blob 標頭格式<br/>重新對內容雜湊" --> GB
```

### Tree ↔ Git Tree

```mermaid
graph LR
    subgraph Noa
        NT["noa tree：<br/>MessagePack [{name, kind, hash}]"]
    end
    subgraph Git
        GT["Git tree：<br/>'&lt;mode&gt; &lt;name&gt;\\0&lt;20-byte-sha1&gt;' 項目"]
    end
    NT -- "TreeEntry::Blob → mode 100644<br/>TreeEntry::Tree → mode 040000<br/>SHA-256 → SHA-1 重新雜湊" --> GT
```

### Snapshot ↔ Git Commit

```mermaid
graph LR
    subgraph "noa Snapshot"
        NS["id: noa_abc123<br/>tree_hash: SHA-256<br/>parents: [noa_...]<br/>author: agent-001<br/>timestamp: 1717592400000000 (µs)<br/>message: 'add feature'"]
    end
    subgraph "Git Commit"
        GC["tree: SHA-1<br/>parent: SHA-1<br/>author: agent-001 &lt;agent@noa&gt;<br/>message: 'add feature'"]
    end
    NS -- "tree_hash 重新雜湊（SHA-256 → SHA-1）<br/>parents 透過 ID 查詢對應<br/>author 以虛擬電子郵件格式化<br/>µs 時間戳截斷為秒" --> GC
```

### Workspace ↔ Git Branch

```mermaid
graph LR
    subgraph Noa
        NW["workspace 'feature-1'<br/>(head: noa_abc123)"]
        ND["workspace 'default'<br/>(head: noa_def456)"]
    end
    subgraph Git
        GB1["branch 'feature-1'<br/>(HEAD: git-sha1)"]
        GB2["branch 'main'<br/>(HEAD: git-sha1)"]
    end
    NW --> GB1
    ND --> GB2
```

### Ref 對應

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

## Push 流程

```mermaid
flowchart TD
    A["1. noa push --remote origin"] --> B["2. 載入從工作區 head 可到達的所有快照"]
    B --> C["3. 將每個快照轉譯為 Git commit"]
    C --> D["4. 將每個 blob/tree 轉譯為 Git 物件"]
    D --> E["5. 透過 gix (gitoxide) 推送到 origin URL"]
    E --> F["6. 更新遠端 refs"]
```

## Pull 流程

```mermaid
flowchart TD
    A["1. noa pull --remote origin"] --> B["2. 透過 gix 擷取 refs"]
    B --> C["3. 對每個新的 Git commit："]
    C --> D["a. 轉譯為 noa snapshot<br/>b. 將 blobs/trees 轉譯為 noa 物件<br/>c. 儲存到本機 redb"]
    D --> E["4. 建立合併快照（本機 head + 遠端 head）"]
    E --> F["5. 更新 workspace head"]
```

## MinIO/S3 後端

適用於沒有 Git 基礎設施的部署：

```mermaid
flowchart TD
    A["noa push --remote s3-remote"] --> B["PUT /bucket/snapshots/noa_abc123 (msgpack)"]
    A --> C["PUT /bucket/blobs/&lt;sha256&gt; (raw bytes)"]
    A --> D["PUT /bucket/trees/&lt;sha256&gt; (msgpack)"]
    A --> E["PUT /bucket/refs/default (snapshot ID text)"]
```

相較於 Git 遠端的優勢：
- 無需 tree/Snapshot 轉譯（原生 noa 格式）
- 直接 blob 儲存（無 pack 檔案額外負擔）
- S3 相容（適用於 AWS、GCS、MinIO、Cloudflare R2）

## 遠端設定

儲存在 `.noa/config`（TOML）中：

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

## 身分驗證

| 後端 | 方法 |
|---------|--------|
| Git HTTPS | 來自 `~/.git-credentials` 的憑證或提示輸入 |
| Git SSH | SSH 代理或金鑰檔案 |
| MinIO/S3 | 存取金鑰 + 秘密金鑰（環境變數或設定檔） |

## 比較：遠端互通方法

| 方法 | 使用方 | 優點 | 缺點 |
|----------|---------|------|------|
| Git 橋接器 (gix) | noa | 通用相容性 | 轉譯額外負擔、SHA-1/SHA-256 不匹配 |
| 原生協定 | Git | 快速、無需轉譯 | 僅適用於 Git |
| WebDAV | SVN | HTTP 標準 | 有限、SVN 專用 |
| REST API | Bitbucket | 現代、靈活 | 需要託管服務 |
| S3 相容儲存 | noa | 可擴展、雲端原生 | 無橋接器則無 Git 互通性 |

noa 同時支援 Git 橋接器（用於相容性）和原生 S3（用於擴展性）。
