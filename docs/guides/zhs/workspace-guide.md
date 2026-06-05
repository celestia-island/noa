# 工作区指南

工作区是隔离的工作上下文，类似于 Git 分支。每个工作区有独立的
head 快照和代理日志。

## 创建工作区

```bash
noa workspace create feature-1
noa workspace create agent-debug --agent bot-42
```

`--agent` 标志将工作区与特定代理 ID 关联。

## 切换工作区

```bash
noa workspace switch feature-1
noa status
# On workspace: feature-1 (head: noa_abc123)
```

## 列出工作区

```bash
noa workspace list
#   default             head: noa_abc123 base: noa_empty
# * feature-1           head: noa_def456 base: noa_abc123
```

`*` 标记表示活动工作区。

## 合并工作区

```bash
noa workspace switch default
noa workspace merge feature-1
# Merged feature-1 into default -> noa_ghi789
```

检测到冲突时：

```
Conflicts detected:
  CONFLICT: src/main.rs
Merged feature-1 into default -> noa_ghi789
```

默认解决策略为 upstream-wins（采纳对方更改）。未来版本将支持手动冲突解决。

## 删除工作区

```bash
noa workspace delete feature-1
# Deleted workspace 'feature-1'
```

不能删除当前活动的工作区。

## 工作流程模式

```
1. noa workspace create feature-1
2. noa workspace switch feature-1
3. (代理写入文件并创建快照)
4. noa workspace switch default
5. noa workspace merge feature-1
6. noa workspace delete feature-1
```

## 多代理模式

每个代理拥有独立的工作区：

```
Agent-001 → workspace agent-001
Agent-002 → workspace agent-002
...
Agent-N   → workspace agent-N
```

每个工作区有独立的代理日志（`.noa/agent-logs/agent-001.log`），
支持零锁并发写入。合并步骤按时间戳排序所有日志以创建统一历史。

> **注意**：redb 使用排他文件锁，因此多个 CLI 进程无法并发打开同一数据库。
> 对于真正的多进程并发，请使用 noa-server HTTP API。
