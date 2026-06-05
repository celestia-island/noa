# 构建 noa

## 前置条件

- Rust 1.75+（stable）
- Python 3.8+（构建脚本用）
- `just` 命令运行器

## 安装

```bash
git clone https://github.com/celestia-island/noa.git
cd noa
just init          # 获取 Rust 依赖
just build-dev     # 开发构建
```

## 开发命令

```bash
just fmt            # 格式化代码
just clippy         # 代码检查
just test           # 运行测试
just check          # 类型检查
```

## 项目结构

```
src/
├── lib.rs          # 库入口
├── error.rs        # 错误类型
├── config.rs       # 配置管理
├── repo.rs         # 仓库生命周期
├── object/         # ObjectStore trait + 实现
├── log/            # AgentLog trait + 实现
├── snapshot/       # 快照引擎
├── workspace/      # 工作区管理器
├── refs.rs         # RefStore trait + 实现
├── merge/          # 合并引擎
├── git/            # Git 兼容层
├── remote.rs       # RemoteBackend trait
└── cli/            # CLI 命令
```
