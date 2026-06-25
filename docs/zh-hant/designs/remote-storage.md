# 遠端儲存後端設計

## 概述

noa 支援可插拔的遠端儲存後端，用於分發和備份依內容定址的物件。所有後端都實作相同的
`ObjectStore` trait，因此快照、tree 和 blob 可以互換地推送至任何已設定的後端。

## 支援的後端

| 後端 | 型別識別碼 | 傳輸方式 | 分發模型 |
|---------|----------------|-----------|-------------------|
| Redb（本機） | —（永遠為本機） | 嵌入式鍵值儲存 | 無 |
| IPFS（Kubo） | `ipfs` | HTTP API | 點對點（DHT、Bitswap） |
| S3 / MinIO | `s3` | S3 相容 API | 集中式物件儲存 |

## 設定

遠端後端以 `[[storage]]` 陣列的形式儲存在 `.noa/config` 中：

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

每個項目都有一個 `name`（用於 CLI 參照）、一個 `type` 判別欄位，以及後端專屬的欄位。
給定型別的未知欄位會被忽略。

## 工廠模式

`create_remote_store(&StorageConfig) -> Box<dyn ObjectStore>` 會檢查
`backend_type` 欄位，並建構適當的實作：

```
type = "ipfs"  →  IpfsObjectStore  （reqwest HTTP 用戶端 → Kubo 守護程序）
type = "s3"    →  MinioObjectStore （aws-sdk-s3 → S3 相容端點）
```

## IPFS CID 橋接器

noa 以十六進位編碼的 SHA-256 雜湊（`BlobId`、`TreeId`）來識別物件。
對於 IPFS，這些會被轉換為 CIDv1（raw codec）以供 API 呼叫使用：

```
CIDv1 bytes = [0x01]           // 版本 1
              [0x55]           // raw codec
              [0x12]           // sha2-256 雜湊函式
              [0x20]           // 32 位元組摘要長度
              [32 bytes hash]

CIDv1 string = "b" + base32_lowercase_nopad(CIDv1 bytes)
```

此轉換是一個純函式 — 相同的內容永遠對應到相同的 CID。此對應不需要守護程序的往返。

## 函式庫選擇：採用 reqwest 而非 ipfs-api-backend-hyper

IPFS 後端直接使用 `reqwest` 對接 Kubo HTTP API，而非使用
`ipfs-api-backend-hyper` crate。理由：

- `aws-sdk-s3`（已是相依性）內部使用 hyper；新增
  `ipfs-api-backend-hyper` 有 hyper 版本衝突的風險
- Kubo API 夠簡單，適合輕量的 REST 呼叫
- `reqwest` 搭配 `rustls-tls` 可避免 OpenSSL 系統相依性

## 推送策略

推送快照時，noa 會遞迴走訪 tree：

1. 對於 tree 中的每個 blob：檢查它是否已存在於遠端 → 若否，則推送它
2. 對於每個子樹：遞迴
3. 推送根 tree
4. 對於 IPFS，使用 `--pin` 時：釘選根 CID 以防止垃圾回收

這確保完整的快照圖被傳輸。本機的 `RedbObjectStore`
永遠是事實來源；遠端後端是分發／備份目標。

## 錯誤處理

後端專屬的錯誤會對應到 `NoaError` 變體：

- `IpfsDaemonUnreachable { endpoint }` — 連線被拒、逾時
- `IpfsError { message }` — API 錯誤回應
- `InvalidCid { cid }` — SHA-256 → CID 轉換失敗
- `ObjectNotFound { id }` — 在網路／儲存上找不到區塊

## 設計決策

### 為什麼使用扁平的設定 struct 而非標記列舉？

TOML 沒有原生的列舉支援。帶有 `type` 判別欄位加上選用後端專屬欄位的扁平 struct，
是最適合 TOML 的做法，並且與現有的 `RemoteConfig` 模式（`name` + `url` + `protocol`）相符。

### 為什麼不與 `[[remotes]]` 合併？

Git 遠端（`RemoteConfig`）和物件儲存（`StorageConfig`）服務於不同目的：
- **遠端**用於 git 協定的 push／pull（原始碼分發）
- **儲存**用於依內容定址的物件分發（快照、blob）

將兩者分開可避免混淆，並允許獨立設定。
