# 物件儲存設計

## 概述

noa 使用受 Git 啟發的依內容定址儲存模型，但具備可插拔的後端架構。物件以 SHA-256 雜湊定址，並以不透明 blob 的形式儲存。

## 物件類型

### Blob

原始檔案內容。以 `SHA256(content)` 識別。

```rust
pub struct BlobId(pub String); // 十六進位編碼的 SHA-256
```

無差異壓縮。每份唯一內容產生恰好一個 blob。重複內容會自動透過雜湊進行去重。

### Tree

目錄列表。將路徑對應到子項目（blob 或子樹）。

```rust
pub struct TreeEntry {
    pub name: String,
    pub kind: TreeEntryKind, // Blob 或 Tree
    pub hash: String,        // 子節點的 SHA-256
}

pub struct TreeId(pub String); // SHA-256(msgpack(entries))
```

Tree 以 MessagePack 序列化，以達到緊湊性和快速反序列化。

## Trait 定義

```rust
#[async_trait]
pub trait ObjectStore: Send + Sync {
    async fn put_blob(&self, data: &[u8]) -> Result<BlobId>;
    async fn get_blob(&self, id: &BlobId) -> Result<Vec<u8>>;
    async fn put_tree(&self, entries: Vec<TreeEntry>) -> Result<TreeId>;
    async fn get_tree(&self, id: &TreeId) -> Result<Vec<TreeEntry>>;
}
```

## 後端實作

### RedbObjectStore（本機）

使用 [redb](https://github.com/cberner/redb) 嵌入式鍵值儲存。

- 兩個資料表：`blobs`（鍵：雜湊位元組，值：內容位元組）和 `trees`（鍵：雜湊位元組，值：msgpack 項目）
- 透過記憶體對應檔案進行零複製讀取
- ACID 交易，具備自動崩潰復原
- 透過 MVCC 實現單寫入者、多讀取者
- 無需外部守護程序

### MinioObjectStore（遠端）

使用透過 `aws-sdk-s3` 的 S3 相容 API。

- 路徑式定址：`<bucket>/blobs/<hash>`、`<bucket>/trees/<hash>`
- 支援任何 S3 相容後端（MinIO、AWS S3、GCS 等）
- 具備指數退避的自動重試
- 適用於分散式部署

## 設計決策

### 為什麼使用 SHA-256 而不是 SHA-1？

Git 使用 SHA-1，該演算法在密碼學上已被破解（SHAttered 攻擊，2017 年）。SHA-256 具有抗碰撞性且廣泛可用。

### 為什麼沒有差異壓縮？

1. **簡潔性**：差異壓縮（Git 的 pack 檔案）增加顯著的複雜度（滑動視窗匹配、薄包、差異鏈）。
2. **寫入效能**：直接 blob 寫入為 O(1)。差異壓縮需要讀取現有物件。
3. **AI 代理工作負載**：代理經常重新生成整個檔案。舊版本是短暫的 — 差異鏈會短且多。
4. **後端卸載**：S3/MinIO 在儲存層處理去重。

### 為什麼 Tree 使用 MessagePack？

- 比 JSON 小 30-50%（對於二進位密集的資料）
- 結構描述靈活（無需 protobuf 定義）
- 透過 `rmp-serde` 提供 Rust 生態系統支援
- 快速反序列化

### 為什麼使用 redb 而不是 SQLite？

- **型別安全**：redb 使用 Rust 泛型來定義資料表
- **效能**：redb 針對 Rust 工作負載最佳化（零複製讀取）
- **簡潔性**：單一相依性，無 C 語言函式庫連結
- **崩潰安全**：redb 的寫入前日誌比 SQLite 的 WAL 模式更簡單

取捨：redb 的社群較小，且工具選項比 SQLite 少。對於 noa 的使用案例（嵌入式二進位儲存），這個取捨是有利的。
