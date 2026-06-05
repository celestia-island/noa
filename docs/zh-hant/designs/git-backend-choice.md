# Git 後端選擇：gix (gitoxide) vs git2 (libgit2)

## 狀態：分析

**目前**：`git2 = "0.19"`（libgit2 的 C 語言繫結）
**提案**：`gix = "0.84"`（純 Rust 的 git 實作）

## 摘要

gix (gitoxide) 是一個成熟的純 Rust Git 實作，具備足夠的功能涵蓋範圍，可取代 noa git 橋接器中的 git2。此次遷移可消除 C 語言相依性 (libgit2)、減少跨編譯摩擦，並提供符合 Rust 慣例的 API。

## 比較矩陣

| 準則 | git2 (libgit2) | gix (gitoxide) |
|-----------|---------------|----------------|
| **語言** | C（透過 git2 crate 提供 Rust 繫結） | 純 Rust |
| **成熟度** | 14 年，經生產環境驗證 | 5 年，活躍開發中 (0.84) |
| **編譯** | 約 15 秒（重新建置），需要 CMake + libgit2-dev | 約 8 秒（重新建置），僅需 cargo |
| **跨編譯** | 困難（需要 C 跨編譯工具鏈） | 簡單（cargo 跨編譯） |
| **API 風格** | C 風格、unsafe 區塊、手動生命週期 | Rust 慣例、借用安全、建構器模式 |
| **物件處理** | git2::Blob、Tree、Commit 透過 ODB | gix::objs::BlobRef、TreeRef、CommitRef |
| **樹遍歷** | 使用 .to_object() 手動迭代器 | 使用委派的 breadthfirst/virtual_roots |
| **遠端 push/pull** | git2::Remote（fetch、push） | gix::remote（connect、fetch、push） |
| **Pack/pack-index** | 內建 | 全面（自有 crate：gix-pack） |
| **Refs** | git2::Reference（讀取/寫入） | gix::refs（完整交易支援） |
| **Config** | 有限（儲存庫層級） | 分層設定（系統、使用者、儲存庫） |
| **SHA-1/256** | 僅 SHA-1 | SHA-1 + SHA-256（實驗性） |
| **記憶體安全** | 風險來自 libgit2 C 語言錯誤 | Rust 保證 |
| **可稽核性** | 需要稽核 libgit2 C 語言程式碼庫 | 僅 Rust，cargo-audit |
| **社群** | 龐大（所有主要 VCS 工具） | 成長中（gitoxide、crates-index-diff 等） |

## noa 的 Git 橋接器需求

目前在 `src/git/` 中的使用：

```rust
// import.rs：
//   - Repository::open()           → gix::open()
//   - repo.head().target()         → gix.head().project_id()
//   - repo.find_commit(oid)        → gix.find_object().try_into_commit()
//   - commit.tree()                → gix.find_object(commit.tree()).try_into_tree()
//   - tree.iter()                  → gix::objs::TreeRefIter
//   - entry.to_object(repo)        → gix.find_object(entry.oid())
//   - obj.kind() === Blob          → obj.kind == ObjectKind::Blob
//   - blob.content()               → blob.data

// translate.rs：
//   - 純位元組層級操作（無外部 git 相依性）

// export.rs：
//   - 目前為 todo!() — push 將使用 gix::remote::connect()
//   - 透過 gix-pack 產生 pack 檔案（如有需要）
```

目前所有 6 個 API 呼叫都有直接的 gix 對應。

## gix 對 noa 的功能涵蓋範圍

| 所需功能 | git2 支援 | gix 支援 | 備註 |
|---------------|-------------|-------------|-------|
| 開啟儲存庫 | ✅ | ✅ | `gix::open()` 或 `gix::ThreadSafeRepository::open()` |
| 讀取 HEAD ref | ✅ | ✅ | `gix.head_ref()` / `gix.head()` |
| 依據 OID 尋找 commit | ✅ | ✅ | `gix.find_object(id)?.try_into_commit()` |
| 從 commit 讀取 tree | ✅ | ✅ | `gix.find_object(commit.tree())?.try_into_tree()` |
| 迭代 tree 項目 | ✅ | ✅ | `tree.iter()` 回傳 `TreeRefIter` |
| 讀取 blob 內容 | ✅ | ✅ | `BlobRef` 上的 `blob.data` |
| 從遠端 fetch | ✅ | ✅ | `gix::remote::connect()` |
| 推送至遠端 | ✅ | ✅ | `gix::remote::connect()` |
| Clone | ✅ | ✅ | `gix::prepare_clone()` |
| Pack 檔案產生 | ✅ | ✅ | `gix-pack` crate |
| SHA-256 支援 | ❌ | ✅（實驗性） | 與 SHA-256 快照相關 |
| 非同步支援 | ❌ | ✅（選擇加入） | 適合 tokio 整合 |

## 可行性

所有目前和計劃中的 git 操作都有 gix 對應。API 對應是直接明瞭的：

```rust
// git2（目前）
let repo = git2::Repository::open(path)?;
let head = repo.head()?;
let commit = repo.find_commit(head.target().unwrap())?;
let tree = commit.tree()?;

// gix（提案）
let repo = gix::open(path)?;
let head = repo.head_ref()?.expect("HEAD not found");
let head_id = head.id().detach();
let commit = repo.find_object(head_id)?.try_into_commit()
    .map_err(|_| NoaError::Remote("not a commit".into()))?;
let tree = repo.find_object(commit.tree())?.try_into_tree()
    .map_err(|_| NoaError::Remote("not a tree".into()))?;
```

## 遷移計畫

### 第一階段：替換 import.rs（唯讀操作）
- 以 gix::ThreadSafeRepository 替換 git2::Repository
- 重新實作樹遍歷
- 執行現有的 git 匯入測試

### 第二階段：替換 translate.rs
- 無需變更（純位元組操作，無 C 語言相依性）

### 第三階段：透過 gix 實作 export.rs
- 使用 gix::remote 進行 push
- 使用 gix::prepare_clone 進行 clone
- 使用 gix-pack 產生 packfile（若伺服器端有需要）

### 第四階段：從 Cargo.toml 移除 git2
- 移除 libgit2 系統相依性
- 驗證跨編譯（x86_64 → aarch64，未來 → wasm）

## 風險評估

| 風險 | 可能性 | 影響 | 緩解措施 |
|------|-----------|--------|------------|
| gix API 變更（0.x 版） | 中 | 低 | 固定版本，適應 API 變更 |
| 缺少進階功能 | 低 | 中 | gix 自 0.50+ 起已有遠端 push/fetch |
| 效能回歸 | 低 | 低 | gix 通常更快（無 C FFI 額外負擔） |
| 社群採用風險 | 低 | 低 | gix 是事實上的 Rust git 函式庫 |
| SHA-256 互通性錯誤 | 中 | 低 | 功能閘控，透過純 translate.rs 繞過 |

## 建議

**遷移至 gix。**其優點（零 C 語言相依性、純 Rust 安全性、更簡單的跨編譯、SHA-256 支援）超過風險（0.x API 穩定性、較小的社群）。此遷移風險低的原因如下：

1. 目前 git2 的使用量極少（import.rs 中僅 6 個 API 呼叫）
2. translate.rs 無需變更
3. export.rs 尚未實作（gix 的綠地開發）
4. gix 是標準的 Rust git 函式庫（被 crates.io 索引所使用）

## 遷移後的相依性

```diff
- git2 = "0.19"           # libgit2 C 語言繫結
+ gix = { version = "0.84", features = ["basic", "index", "pack"] }
```

無新的系統相依性。純 `cargo build`。
