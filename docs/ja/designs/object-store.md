# オブジェクトストレージの設計

## 概要

noaはGitに着想を得たコンテンツアドレス型ストレージモデルを採用していますが、プラグ可能なバックエンドアーキテクチャを備えています。オブジェクトはSHA-256ハッシュでアドレス指定され、不透明なblobとして保存されます。

## オブジェクトタイプ

### Blob

生のファイルコンテンツ。`SHA256(content)`で識別されます。

```rust
pub struct BlobId(pub String); // 16進数エンコードされたSHA-256
```

デルタ圧縮なし。各一意のコンテンツは正確に1つのblobを生成します。重複コンテンツはハッシュによって自動的に重複排除されます。

### ツリー

ディレクトリ一覧。パスを子エントリ（blobまたはサブツリー）にマッピングします。

```rust
pub struct TreeEntry {
    pub name: String,
    pub kind: TreeEntryKind, // Blob または Tree
    pub hash: String,        // 子のSHA-256
}

pub struct TreeId(pub String); // SHA-256(msgpack(entries))
```

ツリーはコンパクトさと高速なデシリアライズのためにMessagePackでシリアライズされます。

## トレイト定義

```rust
#[async_trait]
pub trait ObjectStore: Send + Sync {
    async fn put_blob(&self, data: &[u8]) -> Result<BlobId>;
    async fn get_blob(&self, id: &BlobId) -> Result<Vec<u8>>;
    async fn put_tree(&self, entries: Vec<TreeEntry>) -> Result<TreeId>;
    async fn get_tree(&self, id: &TreeId) -> Result<Vec<TreeEntry>>;
}
```

## バックエンド

### RedbObjectStore (ローカル)

[redb](https://github.com/cberner/redb)組み込みキーバリューストアを使用します。

- 2つのテーブル: `blobs` (キー: ハッシュバイト, 値: コンテンツバイト) と `trees` (キー: ハッシュバイト, 値: msgpackエントリ)
- メモリマップドファイルによるゼロコピー読み取り
- 自動クラッシュリカバリ付きACIDトランザクション
- MVCCによるシングルライター、マルチリーダー
- 外部デーモン不要

### MinioObjectStore (リモート)

`aws-sdk-s3`経由でS3互換APIを使用します。

- パス形式アドレッシング: `<bucket>/blobs/<hash>`, `<bucket>/trees/<hash>`
- あらゆるS3互換バックエンドに対応 (MinIO, AWS S3, GCS等)
- 指数バックオフによる自動リトライ
- 分散デプロイメントに適している

## 設計判断

### SHA-1ではなくSHA-256を選んだ理由

GitはSHA-1を使用していますが、これは暗号学的に破られています (SHAttered攻撃, 2017年)。SHA-256は衝突耐性があり、広く利用可能です。

### デルタ圧縮がない理由

1. **シンプルさ**: デルタ圧縮 (Gitのパックファイル) は大幅な複雑さを追加します (スライディングウィンドウマッチング、thin packs、デルタチェーン)。
2. **書き込みパフォーマンス**: 直接blob書き込みはO(1)。デルタ圧縮には既存オブジェクトの読み取りが必要。
3. **AIエージェントのワークロード**: エージェントは頻繁にファイル全体を再生成します。古いバージョンは一時的 — デルタチェーンは短く多数になります。
4. **バックエンドへのオフロード**: S3/MinIOがストレージ層で重複排除を処理します。

### ツリーにMessagePackを選んだ理由

- JSONよりバイナリデータで30-50%小さい
- スキーマ柔軟性 (protobuf定義不要)
- `rmp-serde`によるRustエコシステム対応
- 高速なデシリアライズ

### SQLiteではなくredbを選んだ理由

- **型安全性**: redbはテーブル定義にRustジェネリクスを使用
- **パフォーマンス**: redbはRustワークロードに最適化 (ゼロコピー読み取り)
- **シンプルさ**: 単一依存関係、Cライブラリリンク不要
- **クラッシュ安全性**: redbの先行書き込みログはSQLiteのWALモードよりシンプル

トレードオフ: redbはSQLiteよりコミュニティが小さく、ツールオプションも少ないです。noaのユースケース（組み込みバイナリストレージ）では、このトレードオフは有利です。
