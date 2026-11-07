# 合併與衝突

noa 的合併是「树級遞歸 + 行級叶子」雙層结构：目錄逐層遞歸，文件級衝突先尝试行級
三路合併（diff3），失败再落结构化衝突。

## 行級三路合併（W2④）

对同一文件的雙方修改，先以公共祖先为基做 diff3 扫描：

- **只有一方觸及的区域**：直接取该方——非重叠编辑自动合併，不再产生整 blob 衝突；
- **重叠或邻接的区域**（與 git 一致，零间隙也算衝突）：生成 git 风格衝突標記块
  （`<<<<<<< ours` / `=======` / `>>>>>>> theirs`），同时保留结构化衝突供面板解决；
- **完全相同或單方未动**：快捷路径直接取已方。

```rust
use libnoa::merge::line_merge::{merge_lines, LineMerge};

match merge_lines(base, ours, theirs) {
    LineMerge::Clean(merged) => { /* 自动解决 */ }
    LineMerge::Conflict { merged, ours, theirs, base } => { /* 结构化衝突 */ }
}
```

## 精確内容 rename（W2④ 刀二）

一方「纯改名」（刪除旧路径、原 blob 内容在新路径原样重现）+ 另一方**修改旧路径**
时，修改自动应用在**新路径**——经典的 rename+edit 场景不再产生「刪除 vs 修改」衝突。

不声明的情形（保守落结构化衝突）：

- 改名同时改了内容（載荷 rename）；
- 雙方各自改到不同目標；
- 目標路径被另一方觸碰（碰撞）；
- rename vs delete（rename 携 base 内容在新路径存活——git rename-aware 語义）。

相似度啟發式（非精確匹配的改名識别）是後续切片。

## 降級語义

行級合併不可用（blob 缺失/損坏/二进制/含独立 CR）时，该文件退回结构化整 blob 衝突
——**绝不**静默丢棄文件，也绝不 500。三方全空的 fixture 形態（存储把缺失 id 物化成
空 blob）同样降級。

## PR 合併流程

`POST /api/v1/repos/{name}/prs/{number}/merge`：

1. 快照三方合併（base = PR base，ours = 目標工作區，theirs = PR head）；
2. 无衝突 → 自动落树 + 推进參照；
3. 有衝突 → 409 + 衝突清單（路径 + ours/theirs/base blob id），面板按
   `ConflictResolution::{Ours, Theirs}` 解决。

## git 兼容性

合併结果满足 git 对象代數：解决後的树可直接回寫 scratch 并回流（回流推送 + 忠實度
校驗全程无转换）。
