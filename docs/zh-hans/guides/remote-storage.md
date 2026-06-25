# 远程存储指南

## 概述

noa 支持多种远程存储后端，用于分发和备份内容寻址对象。后端按仓库配置，并通过统一的 `noa storage` 命令管理。

## 支持的后端

| 后端 | 类型 | 要求 | 用例 |
|---------|------|----------|----------|
| IPFS（Kubo） | `ipfs` | 运行中的 IPFS 守护进程 | 去中心化 P2P 分发 |
| S3 / MinIO | `s3` | S3 兼容端点 | 集中式备份、云存储 |

## 添加存储后端

### IPFS

首先，启动 Kubo 守护进程：

```bash
ipfs daemon &   # 监听 127.0.0.1:5001
```

添加后端：

```bash
# 使用默认值添加 IPFS（endpoint=http://127.0.0.1:5001，gateway=https://ipfs.io）
noa storage add ipfs-local --type ipfs

# 自定义端点和网关
noa storage add ipfs-local --type ipfs \
  --endpoint http://192.168.1.100:5001 \
  --gateway https://dweb.link

# 使用远程固定服务（例如 Pinata）
noa storage add pinata --type ipfs \
  --endpoint https://api.pinata.cloud/psa \
  --auth-token YOUR_PINATA_TOKEN

# 在每次推送时启用自动固定
noa storage add ipfs-local --type ipfs --auto-pin
```

### S3 / MinIO

```bash
# 添加 S3 兼容后端
noa storage add s3-backup --type s3 \
  --endpoint https://s3.us-east-1.amazonaws.com \
  --bucket my-noa-objects \
  --access-key AKIAIOSFODNN7EXAMPLE \
  --secret-key wJalrXUtnFEMI/K7MDENG/bPxRfiCYEXAMPLEKEY \
  --region us-east-1

# 添加本地 MinIO 服务器
noa storage add minio-local --type s3 \
  --endpoint http://localhost:9000 \
  --bucket noa \
  --access-key minioadmin \
  --secret-key minioadmin
```

## 管理后端

```bash
# 列出所有已配置的后端
noa storage list
# ipfs-local    http://127.0.0.1:5001 (ipfs) [gateway=https://ipfs.io]
# s3-backup     https://s3.example.com (s3) [bucket=noa-objects]

# 检查连接状态
noa storage status               # 所有后端
noa storage status ipfs-local    # 特定后端

# 移除后端
noa storage remove s3-backup
```

## 推送快照

将对象推送到远程后端以进行分发或备份：

```bash
# 推送所有快照到特定后端
noa storage push --target ipfs-local

# 推送并固定（仅限 IPFS——防止垃圾回收）
noa storage push --target ipfs-local --pin

# 推送特定快照
noa storage push --target s3-backup --snapshot noa_abc123

# 推送工作区的所有快照
noa storage push --target ipfs-local --workspace feature-auth --pin
```

当配置中设置 `auto_pin = true` 时，`--pin` 是隐含的。你也可以省略 `--target` 来一次推送到所有自动固定后端：

```bash
noa storage push --pin   # 推送到所有 auto_pin=true 的后端
```

## 获取对象

从远程后端下载对象并存储到本地：

```bash
# 按 SHA-256 哈希获取（任意后端）
noa storage fetch ipfs-local 2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824

# 按 CID 获取（仅限 IPFS）
noa storage fetch ipfs-local bafkreihdwdcefgh4dqkjv67uzcmw7ojel6viejvs6buyq7omaygs5sep7m
```

## 推送工作原理

1. **本地优先**：noa 从本地 `RedbObjectStore` 读取对象
2. **递归传输**：对于每个快照，会遍历整个 tree（blob 和子树）。远程不存在的对象会被传输。
3. **内容寻址**：两种后端都使用 SHA-256。对于 IPFS，哈希转换为 CIDv1（raw 编解码器）。对于 S3，哈希用作对象键。
4. **固定**（仅限 IPFS）：推送后，`--pin` 告知守护进程保留对象，防止垃圾回收。

## 配置格式

```toml
# .noa/config

[[storage]]
name = "ipfs-local"
type = "ipfs"
endpoint = "http://127.0.0.1:5001"
gateway = "https://ipfs.io"
auto_pin = true

[[storage]]
name = "s3-backup"
type = "s3"
endpoint = "https://s3.example.com"
bucket = "noa-objects"
access_key = "AKIA..."
secret_key = "..."
region = "us-east-1"
```

## 编程式使用

```rust
use libnoa::config::StorageConfig;
use libnoa::object::{create_remote_store, ObjectStore};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cfg = StorageConfig::ipfs("local", "http://127.0.0.1:5001");
    let store = create_remote_store(&cfg).await?;

    // 远程存储内容
    let blob_id = store.put_blob(b"hello world").await?;
    println!("Stored as: {}", blob_id);

    // 检查存在性
    assert!(store.has_blob(&blob_id).await?);

    // 检索
    let data = store.get_blob(&blob_id).await?;
    assert_eq!(data, b"hello world");

    Ok(())
}
```
