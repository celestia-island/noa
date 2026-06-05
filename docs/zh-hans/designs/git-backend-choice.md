# Git 后端选择：gix (gitoxide) vs git2 (libgit2)

## 状态：分析中

**当前**：`git2 = "0.19"`（libgit2 的 C 绑定）
**提议**：`gix = "0.84"`（纯 Rust Git 实现）

## 概述

gix (gitoxide) 是一个成熟的纯 Rust Git 实现，功能覆盖足以替换 git2 用于 noa 的 Git 桥接。迁移将消除 C 依赖（libgit2），减少交叉编译摩擦，并提供符合 Rust 习惯的 API。

## 对比矩阵

| 标准 | git2 (libgit2) | gix (gitoxide) |
|------|---------------|----------------|
| **语言** | C（Rust 通过 git2 crate 绑定） | 纯 Rust |
| **成熟度** | 14 年，生产验证 | 5 年，活跃开发（0.84） |
| **编译时间** | ~15s（重新构建），需要 CMake + libgit2-dev | ~8s（重新构建），仅 cargo |
| **交叉编译** | 麻烦（需要 C 交叉工具链） | 简单（cargo 交叉编译） |
| **API 风格** | C 风格，unsafe 块，手动生命周期 | Rust 惯用风格，借用安全，构建器模式 |
| **对象处理** | git2::Blob, Tree, Commit 通过 ODB | gix::objs::BlobRef, TreeRef, CommitRef |
| **Tree 遍历** | 手动迭代器 + .to_object() | breadthfirst/virtual_roots + delegate |
| **远程推送/拉取** | git2::Remote (fetch, push) | gix::remote (connect, fetch, push) |
| **Pack/Pack-index** | 内置 | 全面（单独 crate：gix-pack） |
| **Refs** | git2::Reference（读/写） | gix::refs（完整事务支持） |
| **配置** | 有限（仓库级别） | 分层配置（系统、用户、仓库） |
| **SHA-1/256** | 仅 SHA-1 | SHA-1 + SHA-256（实验性） |
| **内存安全** | 来自 libgit2 C bug 的风险 | Rust 保证 |
| **可审计性** | 需要审计 libgit2 C 代码库 | 仅 Rust，cargo-audit |
| **社区** | 庞大（所有主流 VCS 工具） | 增长中（gitoxide, crates-index-diff 等） |

## noa 的 Git 桥接需求

当前在 `src/git/` 中的使用：

```rust
// import.rs:
//   - Repository::open()           → gix::open()
//   - repo.head().target()         → gix.head().project_id()
//   - repo.find_commit(oid)        → gix.find_object().try_into_commit()
//   - commit.tree()                → gix.find_object(commit.tree()).try_into_tree()
//   - tree.iter()                  → gix::objs::TreeRefIter
//   - entry.to_object(repo)        → gix.find_object(entry.oid())
//   - obj.kind() === Blob          → obj.kind == ObjectKind::Blob
//   - blob.content()               → blob.data

// translate.rs:
//   - 纯字节级操作（无外部 git 依赖）
//   - 不需要 gix 或 git2

// export.rs:
//   - 当前为 todo!() — 推送将使用 gix::remote::connect()
//   - 通过 gix-pack 生成 Pack 文件（如需要）
```

所有 6 个当前 API 调用都有直接的 gix 等效项。

## 可行性

所有当前和计划的 git 操作都有 gix 等效项。API 映射直接：

```rust
// git2（当前）
let repo = git2::Repository::open(path)?;
let head = repo.head()?;
let commit = repo.find_commit(head.target().unwrap())?;
let tree = commit.tree()?;

// gix（提议）
let repo = gix::open(path)?;
let head = repo.head_ref()?.expect("HEAD not found");
let head_id = head.id().detach();
let commit = repo.find_object(head_id)?.try_into_commit()
    .map_err(|_| NoaError::Remote("not a commit".into()))?;
let tree = repo.find_object(commit.tree())?.try_into_tree()
    .map_err(|_| NoaError::Remote("not a tree".into()))?;
```

## 迁移计划

### 第一阶段：替换 import.rs（只读操作）
- 用 gix::ThreadSafeRepository 替换 git2::Repository
- 重新实现 tree 遍历
- 运行现有的 Git 导入测试

### 第二阶段：替换 translate.rs
- 无需更改（纯字节操作，无 C 依赖）

### 第三阶段：通过 gix 实现 export.rs
- 使用 gix::remote 进行推送
- 使用 gix::prepare_clone 进行克隆
- 使用 gix-pack 生成 packfile（如需要用于服务端）

### 第四阶段：从 Cargo.toml 中删除 git2
- 放弃 libgit2 系统依赖
- 验证交叉编译（x86_64 → aarch64，未来 → wasm）

## 风险评估

| 风险 | 可能性 | 影响 | 缓解措施 |
|------|--------|------|----------|
| gix API 不兼容变更（0.x） | 中 | 低 | 固定版本，适配 API 变更 |
| 缺少高级功能 | 低 | 中 | gix 自 0.50+ 起支持远程推送/拉取 |
| 性能回退 | 低 | 低 | gix 通常更快（无 C FFI 开销） |
| 社区采用风险 | 低 | 低 | gix 是事实上的 Rust Git 库 |
| SHA-256 互操作 bug | 中 | 低 | 特性门控，通过纯 translate.rs 绕过 |

## 建议

**迁移到 gix。** 好处（零 C 依赖、纯 Rust 安全、更简单的交叉编译、SHA-256 支持）超过风险（0.x API 稳定性、较小的社区）。迁移风险低，因为：

1. 当前 git2 使用量极小（import.rs 中 6 个 API 调用）
2. translate.rs 无需更改
3. export.rs 尚未实现（为 gix 预留绿地）
4. gix 是标准 Rust Git 库（被 crates.io 索引使用）

## 迁移后的依赖

```diff
- git2 = "0.19"           # libgit2 C 绑定
+ gix = { version = "0.84", features = ["basic", "index", "pack"] }
```

无需新的系统依赖。纯 `cargo build` 即可。
