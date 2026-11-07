# 合并与冲突

noa 的合并是「树级递归 + 行级叶子」双层结构：目录逐层递归，文件级冲突先尝试行级
三路合并（diff3），失败再落结构化冲突。

## 行级三路合并（W2④）

对同一文件的双方修改，先以公共祖先为基做 diff3 扫描：

- **只有一方触及的区域**：直接取该方——非重叠编辑自动合并，不再产生整 blob 冲突；
- **重叠或邻接的区域**（与 git 一致，零间隙也算冲突）：生成 git 风格冲突标记块
  （`<<<<<<< ours` / `=======` / `>>>>>>> theirs`），同时保留结构化冲突供面板解决；
- **完全相同或单方未动**：快捷路径直接取已方。

```rust
use libnoa::merge::line_merge::{merge_lines, LineMerge};

match merge_lines(base, ours, theirs) {
    LineMerge::Clean(merged) => { /* 自动解决 */ }
    LineMerge::Conflict { merged, ours, theirs, base } => { /* 结构化冲突 */ }
}
```

## 精确内容 rename（W2④ 第二刀）

一方「纯改名」（删除旧路径、原 blob 内容在新路径原样重现）+ 另一方**修改旧路径**
时，修改自动应用在**新路径**——经典的 rename+edit 场景不再产生「删除 vs 修改」冲突。

不声明的情形（保守落结构化冲突）：

- 改名同时改了内容（载荷 rename）；
- 双方各自改到不同目标；
- 目标路径被另一方触碰（碰撞）；
- rename vs delete（rename 携 base 内容在新路径存活——git rename-aware 语义）。

相似度启发式（非精确匹配的改名识别）是后续切片。

## 降级语义

行级合并不可用（blob 缺失/损坏/二进制/含独立 CR）时，该文件退回结构化整 blob 冲突
——**绝不**静默丢弃文件，也绝不 500。三方全空的 fixture 形态（存储把缺失 id 物化成
空 blob）同样降级。

## PR 合并流程

`POST /api/v1/repos/{name}/prs/{number}/merge`：

1. 快照三方合并（base = PR base，ours = 目标工作区，theirs = PR head）；
2. 无冲突 → 自动落树 + 推进引用；
3. 有冲突 → 409 + 冲突清单（路径 + ours/theirs/base blob id），面板按
   `ConflictResolution::{Ours, Theirs}` 解决。

## git 兼容性

合并结果的 blob 条目与 git 对象代数一致（blob OID 逐字节对应）。但**解决后的树本身
是 noa 原生树**（noa SHA-256 寻址）：它不会自动出现在 scratch 里——要把它推回 origin
需要快照 → git 对象的序列化能力（`export_noa_to_git` 的对象级导出是后续切片；现有的
工作树导出路线可手动完成同样的事）。在此之前，合并结果留在 noa 引用体系内，通过
工作区/快照照常可读。
