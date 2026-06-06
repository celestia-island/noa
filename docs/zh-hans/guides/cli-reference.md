# CLI 命令参考

## `noa init [path]`

初始化新的 `.noa/` 仓库。创建 `noa.redb`、`agent-logs/`、`HEAD` 和 `config`。

```bash
noa init .           # 当前目录
noa init /path/repo  # 指定路径
```

## `noa status`

显示当前工作区和头部快照。

```bash
noa status
# On workspace: default (head: noa_abc123, msg: initial)
```

## `noa log [选项]`

查看快照历史。

| 参数 | 默认值 | 说明 |
|------|--------|------|
| `-w, --workspace` | 当前 HEAD | 按工作区过滤 |
| `-l, --limit` | 20 | 最大显示条数 |

```bash
noa log
noa log --workspace feature-1 --limit 50
```

## `noa snapshot <子命令>`

### `noa snapshot create [-m 消息] [-a 作者]`

从当前工作区的代理日志创建快照。

```bash
noa snapshot create -m "添加登录功能" -a "agent-001"
```

### `noa snapshot list`

列出所有工作区的快照。

### `noa snapshot diff <a> <b>`

显示两个快照之间的文件差异。

```bash
noa snapshot diff noa_abc123 noa_def456
```

## `noa workspace <子命令>`

### `noa workspace create <名称> [--agent <id>]`

从当前 HEAD 分叉创建新工作区。

### `noa workspace switch <名称>`

切换活动工作区（更新 HEAD）。

### `noa workspace list`

列出所有工作区。`*` 标记活动工作区。

### `noa workspace delete <名称>`

删除工作区（不能删除当前活动工作区）。

### `noa workspace merge <来源>`

使用三路合并将另一个工作区合并到当前工作区。

```bash
noa workspace switch default
noa workspace merge feature-1
```

## `noa remote <子命令>`

### `noa remote add <名称> <url>`

添加远程仓库。

### `noa remote remove <名称>`

移除远程仓库。

### `noa remote list`

列出所有已配置的远程仓库。

## `noa push [--remote 名称]`

推送到远程仓库（尚未实现）。

## `noa pull [--remote 名称]`

从远程仓库拉取（尚未实现）。

## `noa fetch [--remote 名称]`

从远程仓库获取但不合并（尚未实现）。

## `noa clone <url> [路径]`

克隆远程仓库（尚未实现）。
