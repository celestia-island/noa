# 入門指南

## 前置需求

- Rust 1.75+（穩定版）
- Python 3.8+（用於建置腳本）
- `just` 指令執行器

## 安裝

```bash
git clone https://github.com/celestia-island/noa.git
cd noa
just init          # 擷取相依性
just build-dev     # 開發版建置
```

`noa` 二進位檔位於 `target/debug/noa`。

## 快速入門

```bash
# 初始化新的儲存庫
noa init .

# 檢查狀態
noa status
# On workspace: default

# 建立工作區
noa workspace create feature-1

# 切換到它
noa workspace switch feature-1

# 建立快照
noa snapshot create -m "initial work"

# 檢視歷史記錄
noa log

# 切換回來並合併
noa workspace switch default
noa workspace merge feature-1

# 管理遠端儲存庫
noa remote add origin https://github.com/example/repo.git
noa remote list
```

## 執行範例

```bash
python3 examples/run_all.py
```

## 開發

```bash
just fmt            # 格式化程式碼
just clippy         # 程式碼檢查
just test           # 執行測試
just check          # 型別檢查
```
