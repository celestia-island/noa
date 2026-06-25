# 遠端儲存指南

## 概述

noa 支援多種遠端儲存後端，用於分發和備份依內容定址的物件。後端以每個儲存庫為單位進行
設定，並透過統一的 `noa storage` 命令進行管理。

## 支援的後端

| 後端 | 型別 | 需求 | 使用案例 |
|---------|------|----------|----------|
| IPFS（Kubo） | `ipfs` | 執行中的 IPFS 守護程序 | 去中心化 P2P 分發 |
| S3 / MinIO | `s3` | S3 相容端點 | 集中式備份、雲端儲存 |

## 新增儲存後端

### IPFS

首先，啟動 Kubo 守護程序：

```bash
ipfs daemon &   # 監聽於 127.0.0.1:5001
```

新增後端：

```bash
# 以預設值新增 IPFS（endpoint=http://127.0.0.1:5001、gateway=https://ipfs.io）
noa storage add ipfs-local --type ipfs

# 自訂 endpoint 與閘道
noa storage add ipfs-local --type ipfs \
  --endpoint http://192.168.1.100:5001 \
  --gateway https://dweb.link

# 使用遠端釘選服務（例如 Pinata）
noa storage add pinata --type ipfs \
  --endpoint https://api.pinata.cloud/psa \
  --auth-token YOUR_PINATA_TOKEN

# 在每次推送時啟用自動釘選
noa storage add ipfs-local --type ipfs --auto-pin
```

### S3 / MinIO

```bash
# 新增 S3 相容後端
noa storage add s3-backup --type s3 \
  --endpoint https://s3.us-east-1.amazonaws.com \
  --bucket my-noa-objects \
  --access-key AKIAIOSFODNN7EXAMPLE \
  --secret-key wJalrXUtnFEMI/K7MDENG/bPxRfiCYEXAMPLEKEY \
  --region us-east-1

# 新增本機 MinIO 伺服器
noa storage add minio-local --type s3 \
  --endpoint http://localhost:9000 \
  --bucket noa \
  --access-key minioadmin \
  --secret-key minioadmin
```

## 管理後端

```bash
# 列出所有已設定的後端
noa storage list
# ipfs-local    http://127.0.0.1:5001 (ipfs) [gateway=https://ipfs.io]
# s3-backup     https://s3.example.com (s3) [bucket=noa-objects]

# 檢查連線狀態
noa storage status               # 所有後端
noa storage status ipfs-local    # 特定後端

# 移除後端
noa storage remove s3-backup
```

## 推送快照

將物件推送至遠端後端以進行分發或備份：

```bash
# 將所有快照推送至特定後端
noa storage push --target ipfs-local

# 推送並釘選（僅限 IPFS — 防止垃圾回收）
noa storage push --target ipfs-local --pin

# 推送特定快照
noa storage push --target s3-backup --snapshot noa_abc123

# 從工作區推送所有快照
noa storage push --target ipfs-local --workspace feature-auth --pin
```

若設定中 `auto_pin = true`，則隱含 `--pin`。您也可以省略 `--target`
一次推送至所有自動釘選後端：

```bash
noa storage push --pin   # 推送至所有 auto_pin=true 的後端
```

## 擷取物件

從遠端後端下載物件並儲存至本機：

```bash
# 依 SHA-256 雜湊擷取（任何後端）
noa storage fetch ipfs-local 2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824

# 依 CID 擷取（僅限 IPFS）
noa storage fetch ipfs-local bafkreihdwdcefgh4dqkjv67uzcmw7ojel6vziejvs6buyq7omaygs5sep7m
```

## Push 的運作方式

1. **本機優先**：noa 從本機 `RedbObjectStore` 讀取物件
2. **遞迴傳輸**：對於每個快照，會走訪整個 tree（blob 與
   子樹）。不存在於遠端的物件會被傳輸。
3. **內容定址**：兩個後端都使用 SHA-256。對於 IPFS，雜湊會
   轉換為 CIDv1（raw codec）。對於 S3，雜湊用作物件鍵。
4. **釘選**（僅限 IPFS）：推送後，`--pin` 告訴守護程序保留
   物件，防止垃圾回收。

## 設定格式

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

## 程式化使用

```rust
use libnoa::config::StorageConfig;
use libnoa::object::{create_remote_store, ObjectStore};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cfg = StorageConfig::ipfs("local", "http://127.0.0.1:5001");
    let store = create_remote_store(&cfg).await?;

    // 遠端儲存內容
    let blob_id = store.put_blob(b"hello world").await?;
    println!("Stored as: {}", blob_id);

    // 檢查存在性
    assert!(store.has_blob(&blob_id).await?);

    // 擷取
    let data = store.get_blob(&blob_id).await?;
    assert_eq!(data, b"hello world");

    Ok(())
}
```
