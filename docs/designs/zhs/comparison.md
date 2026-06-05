# noa vs Git vs SVN vs Bitbucket：横向对比分析

## 概述

noa 是专为 AI 代理工作流设计的版本控制系统。与 Git、SVN 和 Bitbucket（包装 Git/SVN）不同，noa 优化了**非人类参与者的高频并发写入**——数十到数百个 AI 代理同时修改文件而不产生锁竞争。

---

## 功能矩阵对比

| 功能 | noa | Git | SVN | Bitbucket |
|------|-----|-----|-----|-----------|
| **架构** | 嵌入式 KV + 追加日志 | 内容寻址 DAG | 集中式增量存储 | Git/SVN 托管 |
| **并发模型** | 每工作区追加日志（零锁） | 分支级锁定（合并冲突） | 中央服务器序列化 | 同 Git/SVN |
| **合并策略** | 三路合并，默认 upstream-wins | 三路合并，手动解决 | 手动合并 | 同 Git/SVN |
| **快照粒度** | 微秒时间戳，每代理 | 每提交（人工频率） | 每修订版本 | 同 Git/SVN |
| **AI 代理原生** | 是 — 每代理工作区 + 代理日志 | 否 — 为人工工作流设计 | 否 | 否 |
| **存储后端** | 可插拔（redb 本地，MinIO/S3 远程） | Pack 文件 + 松散对象 | Berkeley DB / FSFS | 服务端存储 |
| **分布式** | 是（通过 Git 桥接远程推送/拉取） | 是（原生） | 否（集中式） | 是（托管） |
| **二进制差异** | 内容寻址 blob（无增量） | Pack 级别增量压缩 | 服务端增量 | 同 Git/SVN |
| **锁定** | 写入无锁（追加日志） | 仅建议性锁 | `svn:needs-lock` | 同 Git/SVN |
| **HTTP API** | 内置（noa-server） | git-http-backend | WebDAV | REST API |
| **学习曲线** | 简（6 条命令） | 陡（约 40 条命令） | 中等 | 中等 |

---

## 详细对比

### 1. 并发

**Git**：一个分支 = 同一时间一个写入者。并发写入者创建分歧历史，必须通过合并协调。合并冲突需要人工干预。

**SVN**：中央服务器序列化所有提交。文件级锁定可用但会创建瓶颈。

**noa**：每个代理写入独立的追加日志文件。零锁竞争设计。合并异步进行。

### 2. 数据模型

**Git**：Blob → Tree → Commit → Branch → Ref。SHA-1 内容寻址。不可变对象。分支是可变的指针。

**SVN**：文件/目录 → 修订版本。线性修订号。路径是一等公民。

**noa**：Blob → Tree → Snapshot → Workspace。SHA-256 内容寻址。快照不可变。工作区是带 CAS 更新的可变指针。

关键区别：noa 的 **AgentLog** 层位于代理写入和不可变快照层之间，为高频操作提供缓冲。

### 3. 合并哲学

**Git**：三路合并，需要人工冲突解决。冲突会阻塞进度。

**SVN**：手动合并跟踪。冲突解决是文件级别的。

**noa**：三路合并，可配置自动解决（默认 upstream-wins）。为可以重新应用更改的 AI 代理设计。

### 4. 存储效率

**Git**：带增量压缩的 Pack 文件。优化人工级提交频率。

**SVN**：服务端增量存储。大二进制文件高效。

**noa**：内容寻址 blob，无增量压缩。快照使用 msgpack 编码。权衡：更简单的实现，更快的写入，更大的存储空间。

### 5. 远程互操作

**Git**：原生协议（git://, https://, ssh://）。普遍适用。

**SVN**：svn://, http://。依赖 Apache/Subversion。

**noa**：通过 `gix`（纯 Rust git 实现）的 Git 桥接。可从任何 Git 远程推送/拉取。同时也支持原生 MinIO/S3 后端。

### 6. 访问控制

**Git**：文件系统权限或服务端钩子。

**SVN**：协议内置的基于路径的 ACL。

**Bitbucket**：分支权限、合并检查、代码审查要求。

**noa**：工作区级别隔离。每个代理只能写入其分配的工作区。合并到共享分支需要显式操作。

---

## 何时使用什么

| 场景 | 最佳选择 | 原因 |
|------|----------|------|
| 人类软件开发 | Git | 成熟生态，普遍工具链 |
| AI 代理代码生成（10+ 代理） | noa | 零锁并发写入 |
| 企业合规 + 审计 | SVN | 集中式，基于路径的 ACL |
| 团队协作 + CI/CD | Bitbucket | 内置流水线、PR、审查 |
| AI 代理编排 + 人工审查 | noa → Git 桥接 | 代理在 noa 工作，人工通过 Git 审查 |
| 大型二进制资源 | SVN 或 Git LFS | 二进制增量压缩 |
| 嵌入式 / 边缘设备 | noa | 单文件二进制，redb 嵌入式，无需守护进程 |

---

## 迁移路径

### noa ↔ Git

```bash
# 导出 noa 快照到 Git
noa remote add origin https://github.com/example/repo.git
noa push --remote origin

# 导入 Git 历史到 noa
noa clone https://github.com/example/repo.git
```

`GitTranslator` 在 noa 的 blob/tree 格式和 Git 对象格式之间转换。快照映射为 Git 提交；工作区映射为分支。
