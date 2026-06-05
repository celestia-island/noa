# 对象存储设计

## 概述

noa 使用受 Git 启发的内容寻址存储模型，但具有可插拔后端架构。对象以 SHA-256 哈希为地址，以不透明 blob 形式存储。

## 对象类型

### Blob

原始文件内容。由 `SHA256(content)` 标识。

```rust
pub struct BlobId(pub String); // 十六进制编码的 SHA-256
```

无增量压缩。每个唯一内容产生恰好一个 blob。重复内容通过哈希自动去重。

### Tree

目录列表。将路径映射到子条目（blob 或子树）。

```rust
pub struct TreeEntry {
    pub name: String,
    pub kind: TreeEntryKind, // Blob 或 Tree
    pub hash: String,        // 子树的 SHA-256
}

pub struct TreeId(pub String); // SHA-256(msgpack(entries))
```

Tree 使用 MessagePack 序列化，兼顾紧凑性和快速反序列化。

## Trait 定义

```rust
#[async_trait]
pub trait ObjectStore: Send + Sync {
    async fn put_blob(&self, data: &[u8]) -> Result<BlobId>;
    async fn get_blob(&self, id: &BlobId) -> Result<Vec<u8>>;
    async fn put_tree(&self, entries: Vec<TreeEntry>) -> Result<TreeId>;
    async fn get_tree(&self, id: &TreeId) -> Result<Vec<TreeEntry>>;
}
```

## 后端

### RedbObjectStore（本地）

使用 [redb](https://github.com/cberner/redb) 嵌入式键值存储。

- 两张表：`blobs`（键：哈希字节，值：内容字节）和 `trees`（键：哈希字节，值：msgpack 条目）
- 通过内存映射文件实现零拷贝读取
- ACID 事务，自动崩溃恢复
- 通过 MVCC 实现单写入者、多读取者
- 无需外部守护进程

### MinioObjectStore（远程）

通过 `aws-sdk-s3` 使用 S3 兼容 API。

- 路径式寻址：`<bucket>/blobs/<hash>`, `<bucket>/trees/<hash>`
- 支持任何 S3 兼容后端（MinIO、AWS S3、GCS 等）
- 带指数回退的自动重试
- 适用于分布式部署

## 设计决策

### 为什么用 SHA-256 而非 SHA-1？

Git 使用 SHA-1，该算法已被密码学破解（SHAttered 攻击，2017 年）。SHA-256 具有抗碰撞性，且广泛可用。

### 为什么没有增量压缩？

1. **简洁性**：增量压缩（Git 的 Pack 文件）增加显著复杂度（滑动窗口匹配、thin Pack、delta 链）
2. **写入性能**：直接 blob 写入是 O(1)。增量压缩需要读取已有对象
3. **AI 代理工作负载**：代理频繁重新生成完整文件。旧版本是短暂的——delta 链会短而多
4. **后端卸载**：S3/MinIO 在存储层处理去重

### 为什么 Tree 用 MessagePack？

- 二进制数据比 JSON 小 30-50%
- 模式灵活（无需 protobuf 定义）
- 通过 `rmp-serde` 支持 Rust 生态
- 快速反序列化

### 为什么用 redb 而非 SQLite？

- **类型安全**：redb 使用 Rust 泛型定义表
- **性能**：redb 为 Rust 工作负载优化（零拷贝读取）
- **简洁性**：单一依赖，无 C 库链接
- **崩溃安全**：redb 的预写日志比 SQLite 的 WAL 模式更简单

权衡：redb 的社区比 SQLite 小，工具选项也较少。对于 noa 的用例（嵌入式二进制存储），这种权衡是有利的。
