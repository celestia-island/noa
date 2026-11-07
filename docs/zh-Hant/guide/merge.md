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

合併結果滿足 git 物件代數：解決後的樹可直接回寫 scratch 並回流（回流推送 + 忠實度
校驗全程無轉換）。
