# 合併與衝突

noa 的合併是「樹層級遞迴 + 行層級葉子」的雙層結構：目錄逐層遞迴，檔案層級衝突先嘗試行層級
三路合併（diff3），失敗再落入結構化衝突。

## 行層級三路合併（W2④）

對同一檔案的雙方修改，先以共同祖先為基進行 diff3 掃描：

- **只有一方觸及的區域**：直接取該方——非重疊的編輯自動合併，不再產生整 blob 衝突；
- **重疊或相鄰的區域**（與 git 一致，零間隙也算衝突）：產生 git 風格的衝突標記塊
  （`<<<<<<< ours` / `=======` / `>>>>>>> theirs`），同時保留結構化衝突供面板解決；
- **完全相同或單方未動**：走捷徑直接取該方。

```rust
use libnoa::merge::line_merge::{merge_lines, LineMerge};

match merge_lines(base, ours, theirs) {
    LineMerge::Clean(merged) => { /* 自動解決 */ }
    LineMerge::Conflict { merged, ours, theirs, base } => { /* 結構化衝突 */ }
}
```

## 精確內容 rename（W2④ 第二刀）

當一方「純改名」（刪除舊路徑、原 blob 內容在新路徑原樣重現）+ 另一方**修改舊路徑**
時，修改會自動套用在新路徑——經典的 rename+edit 場景不再產生「刪除 vs 修改」衝突。

不宣告的情形（保守落入結構化衝突）：

- 改名同時也改了內容（payload rename）；
- 雙方各自改到不同目標；
- 目標路徑被另一方觸碰（碰撞）；
- rename vs delete（rename 攜帶 base 內容在新路徑存活——git rename-aware 語意）。

相似度啟發式（非精確比對的改名辨識）是後續切片。

## 降級語意

行層級合併不可用（blob 缺失/損壞/二進位/含獨立 CR）時，該檔案退回結構化整 blob 衝突
——**絕不**靜默捨棄檔案，也絕不回 500。三方全空的 fixture 形態（儲存層把缺失 id 具體化為
空 blob）同樣降級。

## PR 合併流程

`POST /api/v1/repos/{name}/prs/{number}/merge`：

1. 快照三方合併（base = PR base，ours = 目標工作區，theirs = PR head）；
2. 無衝突 → 自動寫入樹 + 推進引用；
3. 有衝突 → 409 + 衝突清單（路徑 + ours/theirs/base blob id），面板按
   `ConflictResolution::{Ours, Theirs}` 解決。

## git 相容性

合併結果的 blob 條目與 git 物件代數一致（blob OID 逐位元組對應）。但**解決後的樹本身
是 noa 原生樹**（noa SHA-256 定址）：它不會自動出現在 scratch 裸儲存庫裡——要把它推回
origin 需要快照 → git 物件的序列化能力（`export_noa_to_git` 的物件級匯出是後續切片；
現有的工作樹匯出路線可手動完成同樣的事）。在此之前，合併結果留在 noa 引用體系內，透過
工作區/快照照常可讀。
