# Gitバックエンドの選択: gix (gitoxide) vs git2 (libgit2)

## ステータス: 分析

**現在**: `git2 = "0.19"` (libgit2のCバインディング)
**提案**: `gix = "0.84"` (純粋RustのGit実装)

## 概要

gix (gitoxide)は成熟した純粋RustのGit実装であり、noaのGitブリッジのgit2を置き換えるのに十分な機能カバレッジを持っています。この移行により、C依存関係(libgit2)が排除され、クロスコンパイルの摩擦が軽減され、慣用的なRust APIが提供されます。

## 比較マトリックス

| 基準 | git2 (libgit2) | gix (gitoxide) |
|-----------|---------------|----------------|
| **言語** | C (git2クレート経由のRustバインディング) | 純粋Rust |
| **成熟度** | 14年、本番環境で実証済み | 5年、活発な開発中 (0.84) |
| **コンパイル** | ~15秒 (再ビルド)、CMake + libgit2-devが必要 | ~8秒 (再ビルド)、cargoのみ |
| **クロスコンパイル** | 困難 (Cクロスツールチェーンが必要) | 簡単 (cargoクロスコンパイル) |
| **APIスタイル** | C風、unsafeブロック、手動ライフタイム | Rust慣用的、借用安全、ビルダーパターン |
| **オブジェクト処理** | git2::Blob, Tree, Commit (ODB経由) | gix::objs::BlobRef, TreeRef, CommitRef |
| **ツリー走査** | .to_object()を使用した手動イテレータ | デリゲート付きのbreadthfirst/virtual_roots |
| **リモートpush/pull** | git2::Remote (fetch, push) | gix::remote (connect, fetch, push) |
| **Pack/pack-index** | 組み込み | 包括的 (独自クレート: gix-pack) |
| **Refs** | git2::Reference (読み取り/書き込み) | gix::refs (完全なトランザクションサポート) |
| **設定** | 限定的 (リポジトリレベル) | 階層化設定 (システム、ユーザー、リポジトリ) |
| **SHA-1/256** | SHA-1のみ | SHA-1 + SHA-256 (実験的) |
| **メモリ安全性** | libgit2のCバグのリスク | Rustの保証 |
| **監査可能性** | libgit2のCコードベースの監査が必要 | Rustのみ、cargo-audit |
| **コミュニティ** | 大規模 (すべての主要VCSツール) | 成長中 (gitoxide, crates-index-diff等) |

## noaのGitブリッジ要件

`src/git/`での現在の使用状況:

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
//   - 純粋なバイトレベル操作 (外部git非依存)

// export.rs:
//   - 現在はtodo!() — pushはgix::remote::connect()を使用
//   - gix-pack経由のパックファイル生成 (必要な場合)
```

現在の6つのAPI呼び出しすべてに直接のgix相当があります。

## noa向けgix機能カバレッジ

| 必要な機能 | git2対応 | gix対応 | 備考 |
|---------------|-------------|-------------|-------|
| リポジトリを開く | ✅ | ✅ | `gix::open()` または `gix::ThreadSafeRepository::open()` |
| HEAD参照の読み取り | ✅ | ✅ | `gix.head_ref()` / `gix.head()` |
| OIDでコミットを検索 | ✅ | ✅ | `gix.find_object(id)?.try_into_commit()` |
| コミットからツリーを読み取り | ✅ | ✅ | `gix.find_object(commit.tree())?.try_into_tree()` |
| ツリーエントリの反復 | ✅ | ✅ | `tree.iter()`が`TreeRefIter`を返す |
| blobコンテンツの読み取り | ✅ | ✅ | `BlobRef`の`blob.data` |
| リモートからフェッチ | ✅ | ✅ | `gix::remote::connect()` |
| リモートへプッシュ | ✅ | ✅ | `gix::remote::connect()` |
| クローン | ✅ | ✅ | `gix::prepare_clone()` |
| パックファイル生成 | ✅ | ✅ | `gix-pack`クレート |
| SHA-256対応 | ❌ | ✅ (実験的) | SHA-256スナップショットに関連 |
| 非同期対応 | ❌ | ✅ (オプトイン) | tokio統合に適している |

## 実現可能性

現在および計画中のすべてのgit操作にgix相当が存在します。APIマッピングは直接的です:

```rust
// git2 (現在)
let repo = git2::Repository::open(path)?;
let head = repo.head()?;
let commit = repo.find_commit(head.target().unwrap())?;
let tree = commit.tree()?;

// gix (提案)
let repo = gix::open(path)?;
let head = repo.head_ref()?.expect("HEAD not found");
let head_id = head.id().detach();
let commit = repo.find_object(head_id)?.try_into_commit()
    .map_err(|_| NoaError::Remote("not a commit".into()))?;
let tree = repo.find_object(commit.tree())?.try_into_tree()
    .map_err(|_| NoaError::Remote("not a tree".into()))?;
```

## 移行計画

### フェーズ1: import.rsの置き換え (読み取り専用操作)
- git2::Repositoryをgix::ThreadSafeRepositoryに置き換え
- ツリー走査の再実装
- 既存のgitインポートテストを実行

### フェーズ2: translate.rsの置き換え
- 変更不要 (純粋なバイト操作、C非依存)

### フェーズ3: gix経由でexport.rsを実装
- プッシュにgix::remoteを使用
- クローンにgix::prepare_cloneを使用
- パックファイル生成にgix-packを使用 (サーバーサイドで必要な場合)

### フェーズ4: Cargo.tomlからgit2を削除
- libgit2システム依存関係を削除
- クロスコンパイルの検証 (x86_64 → aarch64, 将来的に → wasm)

## リスク評価

| リスク | 可能性 | 影響 | 緩和策 |
|------|-----------|--------|------------|
| gix APIの破壊的変更 (0.x) | 中 | 低 | バージョンを固定、API変更に適応 |
| 高度な機能の欠如 | 低 | 中 | gixは0.50以降リモートpush/fetchをサポート |
| パフォーマンス低下 | 低 | 低 | gixの方が高速な場合が多い (C FFIオーバーヘッドなし) |
| コミュニティ採用リスク | 低 | 低 | gixは事実上のRust gitライブラリ |
| SHA-256相互運用バグ | 中 | 低 | フィーチャーゲート、純粋translate.rs経由でバイパス |

## 推奨

**gixに移行する。** 利点（C依存関係ゼロ、純粋Rustの安全性、クロスコンパイルの容易さ、SHA-256対応）がリスク（0.x APIの安定性、小規模なコミュニティ）を上回ります。移行は以下の理由で低リスクです:

1. 現在のgit2使用は最小限 (import.rsの6つのAPI呼び出し)
2. translate.rsは変更不要
3. export.rsは未実装 (gixの新規実装)
4. gixは標準的なRust gitライブラリ (crates.ioインデックスで使用)

## 移行後の依存関係

```diff
- git2 = "0.19"           # libgit2 Cバインディング
+ gix = { version = "0.84", features = ["basic", "index", "pack"] }
```

新しいシステム依存関係はありません。純粋な`cargo build`。
