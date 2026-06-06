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

```mermaid
graph TD
    SRC["src/"] --> LIB["lib.rs<br/>（库入口）"]
    SRC --> ERR["error.rs<br/>（错误类型）"]
    SRC --> CFG["config.rs<br/>（配置管理）"]
    SRC --> REPO["repo.rs<br/>（仓库生命周期）"]
    SRC --> OBJ["object/<br/>（ObjectStore trait + 实现）"]
    SRC --> LOG["log/<br/>（AgentLog trait + 实现）"]
    SRC --> SNAP["snapshot/<br/>（快照引擎）"]
    SRC --> WS["workspace/<br/>（工作区管理器）"]
    SRC --> REFS["refs.rs<br/>（RefStore trait + 实现）"]
    SRC --> MERGE["merge/<br/>（合并引擎）"]
    SRC --> GIT["git/<br/>（Git 兼容层）"]
    SRC --> REMOTE["remote.rs<br/>（RemoteBackend trait）"]
    SRC --> CLI["cli/<br/>（CLI 命令）"]
```
