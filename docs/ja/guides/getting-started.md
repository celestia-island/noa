# はじめに

## 前提条件

- Rust 1.75+ (安定版)
- Python 3.8+ (ビルドスクリプト用)
- `just`コマンドランナー

## インストール

```bash
git clone https://github.com/celestia-island/noa.git
cd noa
just init          # 依存関係を取得
just build-dev     # 開発ビルド
```

`noa`バイナリは`target/debug/noa`にあります。

## クイックスタート

```bash
# 新しいリポジトリを初期化
noa init .

# 状態を確認
noa status
# On workspace: default

# ワークスペースを作成
noa workspace create feature-1

# 切り替え
noa workspace switch feature-1

# スナップショットを作成
noa snapshot create -m "initial work"

# 履歴を表示
noa log

# 戻ってマージ
noa workspace switch default
noa workspace merge feature-1

# リモートを管理
noa remote add origin https://github.com/example/repo.git
noa remote list
```

## サンプルの実行

```bash
python3 examples/run_all.py
```

## 開発

```bash
just fmt            # コードのフォーマット
just clippy         # lint
just test           # テストの実行
just check          # 型チェック
```
