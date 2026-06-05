# 建置 noa

## 前置需求

- Rust 1.75+（穩定版）
- Python 3.8+（用於建置腳本）
- `just` 指令執行器

## 設定

```bash
git clone https://github.com/celestia-island/noa.git
cd noa
just init          # 擷取 Rust 相依性
just build-dev     # 開發版建置
```

## 開發

```bash
just fmt            # 格式化程式碼
just clippy         # 程式碼檢查
just test           # 執行測試
just check          # 型別檢查
```

## 專案結構

```mermaid
graph TD
    SRC["src/"] --> LIB["lib.rs<br/>（函式庫根）"]
    SRC --> ERR["error.rs<br/>（錯誤類型）"]
    SRC --> CFG["config.rs<br/>（設定）"]
    SRC --> REPO["repo.rs<br/>（儲存庫生命週期）"]
    SRC --> OBJ["object/<br/>（ObjectStore trait + 實作）"]
    SRC --> LOG["log/<br/>（AgentLog trait + 實作）"]
    SRC --> SNAP["snapshot/<br/>（快照引擎）"]
    SRC --> WS["workspace/<br/>（工作區管理器）"]
    SRC --> REFS["refs.rs<br/>（RefStore trait + 實作）"]
    SRC --> MERGE["merge/<br/>（合併引擎）"]
    SRC --> GIT["git/<br/>（Git 相容性）"]
    SRC --> REMOTE["remote.rs<br/>（RemoteBackend trait）"]
    SRC --> CLI["cli/<br/>（CLI 指令）"]
```
