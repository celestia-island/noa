# 快速入门

## 前置条件

- Rust 1.75+（stable）
- Python 3.8+（构建脚本用）
- `just` 命令运行器

## 安装

```bash
git clone https://github.com/celestia-island/noa.git
cd noa
just init          # 获取依赖
just build-dev     # 开发构建
```

`noa` 二进制文件位于 `target/debug/noa`。

## 快速上手

```bash
# 初始化仓库
noa init .

# 查看状态
noa status
# On workspace: default

# 创建工作区
noa workspace create feature-1

# 切换工作区
noa workspace switch feature-1

# 创建快照
noa snapshot create -m "初始工作"

# 查看历史
noa log

# 切回并合并
noa workspace switch default
noa workspace merge feature-1

# 管理远程
noa remote add origin https://github.com/example/repo.git
noa remote list
```

## 运行示例

```bash
python3 examples/run_all.py
```

## 开发命令

```bash
just fmt            # 格式化代码
just clippy         # 代码检查
just test           # 运行测试
just check          # 类型检查
```
