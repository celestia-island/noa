# 工作區隔離設計

## 概述

工作區為代理和人類提供隔離的工作環境。每個工作區擁有獨立的狀態（head 快照、代理日誌），同時共享底層的物件儲存。

## 工作區結構

```rust
pub struct Workspace {
    pub name: String,
    pub head: SnapshotId,     // 目前快照
    pub base: SnapshotId,     // 從父工作區分支的分岔點
    pub agent_id: Option<String>,  // 關聯的代理
    pub created_at: u64,
    pub updated_at: u64,
}
```

## 工作區生命週期

```mermaid
flowchart LR
    A["create"] --> B["switch"]
    B --> C["（代理寫入 + 建立快照）"]
    C --> D["merge"]
    D --> E["delete"]
```

### 建立

```bash
noa workspace create feature-1
```

1. 讀取目前工作區的 head 快照 → 成為 `base`
2. 新工作區：`head = base`（繼承目前狀態）
3. 建立代理日誌檔案：`agent-logs/feature-1.log`
4. 在 WorkspaceStore 中註冊

### 切換

```bash
noa workspace switch feature-1
```

1. 驗證工作區存在
2. 將工作區名稱寫入 `.noa/HEAD`
3. 將先前的工作區儲存到 `.noa/ORIG_HEAD`

### 合併

```bash
noa workspace merge feature-1
```

1. 三方合併：base → ours（目前）vs theirs（feature-1）
2. 建立合併快照，以兩者作為父節點
3. 更新目前工作區的 head

### 刪除

```bash
noa workspace delete feature-1
```

1. 驗證不是使用中的工作區
2. 從儲存中移除工作區項目
3. 刪除代理日誌檔案
4. 物件保留（共享、依內容定址）

## HEAD 檔案

`.noa/HEAD` 包含使用中的工作區名稱：

```
feature-1
```

`.noa/ORIG_HEAD` 包含先前的工作區（用於復原）：

```
default
```

## 工作區儲存

工作區儲存在 redb 中：

```
資料表：workspaces
  鍵：   "feature-1"（工作區名稱，型別為 &str）
  值：   msgpack(Workspace)，型別為 &[u8]
```

Head 更新使用 CAS（比較並交換）：

```rust
async fn update_head(&self, name: &str, expected: &SnapshotId, new: &SnapshotId) -> Result<()>
```

這可防止多個處理程序同時嘗試更新同一工作區時發生遺失更新。

## 與 Git 分支的比較

| 面向 | noa 工作區 | Git 分支 |
|--------|---------------|------------|
| 儲存 | redb 資料表項目 | ref 檔案（`.git/refs/heads/`） |
| 隔離 | 自有代理日誌檔案 | 共享索引 + 工作樹 |
| 切換 | 原子性 HEAD 寫入 | 工作樹檢出（檔案 I/O） |
| 建立 | O(1) — 僅詮釋資料 | O(1) — 輕量 |
| 刪除 | 從儲存中移除 | 刪除 ref，可選擇修剪 |
| 代理繫結 | 可選的 agent_id 欄位 | 無對應 |
| Base 追蹤 | 明確的 base 欄位 | 隱含（merge base） |

## 與 SVN 分支的比較

| 面向 | noa 工作區 | SVN 分支 |
|--------|---------------|------------|
| 儲存 | KV 項目 | 完整目錄複製 |
| 建立 | O(1) 詮釋資料 | O(n) 檔案複製 |
| 隔離 | 邏輯（共享物件） | 物理（獨立目錄） |
| 合併追蹤 | 父節點 DAG | svn:mergeinfo 屬性 |

## 設計理念

### 為什麼使用工作區而不是分支？

1. **代理識別**：工作區攜帶 `agent_id` 欄位用於歸屬
2. **代理日誌隔離**：每個工作區有專屬的日誌檔案
3. **無工作樹**：noa 不維護檢出 — 僅有快照
4. **明確的 base**：`base` 欄位可快速計算 merge-base

### 為什麼沒有工作樹檢出？

Git 分支需要工作樹檢出（每個切換的檔案都需要檔案 I/O）。noa 工作區僅切換一個指標 — 代理日誌和快照參照。無論儲存庫大小，這都是 O(1) 操作。

檔案實體化（檢出）在代理需要讀取或寫入實際檔案時單獨進行，使用快照的 tree 作為真實來源。
