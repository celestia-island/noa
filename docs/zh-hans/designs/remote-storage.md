# 远程存储后端设计

## 概述

noa 支持可插拔的远程存储后端，用于分发和备份内容寻址对象。所有后端都实现相同的 `ObjectStore` trait，因此快照、tree 和 blob 可以互换地推送到任何已配置的后端。

## 支持的后端

| 后端 | 类型标识符 | 传输方式 | 分发模型 |
|---------|----------------|-----------|-------------------|
| Redb（本地） | —（始终为本地） | 嵌入式键值存储 | 无 |
| IPFS（Kubo） | `ipfs` | HTTP API | 点对点（DHT、Bitswap） |
| S3 / MinIO | `s3` | S3 兼容 API | 集中式对象存储 |

## 配置

远程后端以 `[[storage]]` 数组形式存储在 `.noa/config` 中：

```toml
[[storage]]
name = "ipfs-local"
type = "ipfs"
endpoint = "http://127.0.0.1:5001"
gateway = "https://ipfs.io"
auto_pin = false

[[storage]]
name = "s3-backup"
type = "s3"
endpoint = "https://s3.example.com"
bucket = "noa-objects"
access_key = "AKIA..."
secret_key = "..."
region = "us-east-1"
```

每个条目有一个 `name`（用于 CLI 引用）、一个 `type` 区分字段，以及后端特定的字段。给定类型的未知字段将被忽略。

## 工厂模式

`create_remote_store(&StorageConfig) -> Box<dyn ObjectStore>` 检查 `backend_type` 字段并构造相应的实现：

```
type = "ipfs"  →  IpfsObjectStore  (reqwest HTTP 客户端 → Kubo 守护进程)
type = "s3"    →  MinioObjectStore (aws-sdk-s3 → S3 兼容端点)
```

## IPFS CID 桥接

noa 通过十六进制编码的 SHA-256 哈希（`BlobId`、`TreeId`）标识对象。对于 IPFS，这些哈希会转换为 CIDv1（raw 编解码器）以进行 API 调用：

```
CIDv1 字节 = [0x01]           // 版本 1
              [0x55]           // raw 编解码器
              [0x12]           // sha2-256 哈希函数
              [0x20]           // 32 字节摘要长度
              [32 字节哈希]

CIDv1 字符串 = "b" + base32_lowercase_nopad(CIDv1 字节)
```

此转换是纯函数——相同的内容始终映射到相同的 CID。映射过程无需守护进程往返。

## 库选择：使用 reqwest 而非 ipfs-api-backend-hyper

IPFS 后端直接使用 `reqwest` 访问 Kubo HTTP API，而非 `ipfs-api-backend-hyper` crate。原因：

- `aws-sdk-s3`（已是依赖项）内部使用 hyper；添加 `ipfs-api-backend-hyper` 有导致 hyper 版本冲突的风险
- Kubo API 足够简单，适合轻量级 REST 调用
- `reqwest` 配合 `rustls-tls` 可避免 OpenSSL 系统依赖

## 推送策略

推送快照时，noa 会递归遍历 tree：

1. 对于 tree 中的每个 blob：检查是否已远程存在→若不存在，则推送它
2. 对于每个子树：递归处理
3. 推送根 tree
4. 对于 IPFS，若指定 `--pin`：固定根 CID 以防止垃圾回收

这确保了完整的快照图被传输。本地 `RedbObjectStore` 始终是事实来源；远程后端是分发/备份目标。

## 错误处理

后端特定的错误映射到 `NoaError` 变体：

- `IpfsDaemonUnreachable { endpoint }` — 连接被拒绝、超时
- `IpfsError { message }` — API 错误响应
- `InvalidCid { cid }` — SHA-256 → CID 转换失败
- `ObjectNotFound { id }` — 在网络/存储中未找到块

## 设计决策

### 为什么使用扁平配置结构而非标签枚举？

TOML 没有原生的枚举支持。带有 `type` 区分字段加上可选后端特定字段的扁平结构是最适合 TOML 的方式，并且与现有的 `RemoteConfig` 模式（`name` + `url` + `protocol`）保持一致。

### 为什么不与 `[[remotes]]` 合并？

Git 远程（`RemoteConfig`）和对象存储（`StorageConfig`）服务于不同目的：
- **远程**用于 git 协议的推送/拉取（源代码分发）
- **存储**用于内容寻址对象分发（快照、blob）

保持它们独立可以避免混淆，并允许独立配置。
