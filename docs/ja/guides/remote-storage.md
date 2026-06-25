# リモートストレージガイド

## 概要

noaは、コンテンツアドレス型オブジェクトの配布とバックアップのための複数のリモートストレージバックエンドをサポートしています。バックエンドはリポジトリごとに設定され、統一された `noa storage` コマンドで管理されます。

## サポートされているバックエンド

| バックエンド | 型 | 必要なもの | ユースケース |
|---------|------|----------|----------|
| IPFS (Kubo) | `ipfs` | 実行中のIPFSデーモン | 非中央集権型P2P配布 |
| S3 / MinIO | `s3` | S3互換エンドポイント | 中央集権型バックアップ、クラウドストレージ |

## ストレージバックエンドの追加

### IPFS

まず、Kuboデーモンを起動します:

```bash
ipfs daemon &   # 127.0.0.1:5001でリッスン
```

バックエンドを追加します:

```bash
# デフォルトでIPFSを追加 (endpoint=http://127.0.0.1:5001, gateway=https://ipfs.io)
noa storage add ipfs-local --type ipfs

# エンドポイントとゲートウェイをカスタマイズ
noa storage add ipfs-local --type ipfs \
  --endpoint http://192.168.1.100:5001 \
  --gateway https://dweb.link

# リモートピニングサービスを使用 (例: Pinata)
noa storage add pinata --type ipfs \
  --endpoint https://api.pinata.cloud/psa \
  --auth-token YOUR_PINATA_TOKEN

# プッシュのたびに自動ピニングを有効化
noa storage add ipfs-local --type ipfs --auto-pin
```

### S3 / MinIO

```bash
# S3互換バックエンドを追加
noa storage add s3-backup --type s3 \
  --endpoint https://s3.us-east-1.amazonaws.com \
  --bucket my-noa-objects \
  --access-key AKIAIOSFODNN7EXAMPLE \
  --secret-key wJalrXUtnFEMI/K7MDENG/bPxRfiCYEXAMPLEKEY \
  --region us-east-1

# ローカルのMinIOサーバーを追加
noa storage add minio-local --type s3 \
  --endpoint http://localhost:9000 \
  --bucket noa \
  --access-key minioadmin \
  --secret-key minioadmin
```

## バックエンドの管理

```bash
# すべての設定済みバックエンドを一覧表示
noa storage list
# ipfs-local    http://127.0.0.1:5001 (ipfs) [gateway=https://ipfs.io]
# s3-backup     https://s3.example.com (s3) [bucket=noa-objects]

# 接続ステータスを確認
noa storage status               # すべてのバックエンド
noa storage status ipfs-local    # 特定のバックエンド

# バックエンドを削除
noa storage remove s3-backup
```

## スナップショットのプッシュ

オブジェクトを配布またはバックアップのためにリモートバックエンドにプッシュします:

```bash
# すべてのスナップショットを特定のバックエンドにプッシュ
noa storage push --target ipfs-local

# プッシュしてピン留め (IPFSのみ — ガベージコレクションを防ぐ)
noa storage push --target ipfs-local --pin

# 特定のスナップショットをプッシュ
noa storage push --target s3-backup --snapshot noa_abc123

# ワークスペースからすべてのスナップショットをプッシュ
noa storage push --target ipfs-local --workspace feature-auth --pin
```

設定で `auto_pin = true` の場合、`--pin` が暗黙となります。`--target` を省略することで、すべての自動ピン留めバックエンドに一度にプッシュすることもできます:

```bash
noa storage push --pin   # auto_pin=trueのすべてのバックエンドにプッシュ
```

## オブジェクトの取得

リモートバックエンドからオブジェクトをダウンロードしてローカルに保存します:

```bash
# SHA-256ハッシュで取得 (任意のバックエンド)
noa storage fetch ipfs-local 2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824

# CIDで取得 (IPFSのみ)
noa storage fetch ipfs-local bafkreihdwdcefgh4dqkjv67uzcmw7ojel6viejvs6buyq7omaygs5sep7m
```

## プッシュの仕組み

1. **ローカル優先**: noaはローカルの `RedbObjectStore` からオブジェクトを読み取る
2. **再帰転送**: 各スナップショットについて、ツリー全体 (blobとサブツリー) が走査される。リモートに存在しないオブジェクトが転送される。
3. **コンテンツアドレッシング**: 両バックエンドともSHA-256を使用。IPFSではハッシュはCIDv1 (rawコーデック) に変換される。S3ではハッシュがオブジェクトキーとして使用される。
4. **ピン留め** (IPFSのみ): プッシュ後、`--pin` はデーモンにオブジェクトを保持するよう指示し、ガベージコレクションを防ぐ。

## 設定形式

```toml
# .noa/config

[[storage]]
name = "ipfs-local"
type = "ipfs"
endpoint = "http://127.0.0.1:5001"
gateway = "https://ipfs.io"
auto_pin = true

[[storage]]
name = "s3-backup"
type = "s3"
endpoint = "https://s3.example.com"
bucket = "noa-objects"
access_key = "AKIA..."
secret_key = "..."
region = "us-east-1"
```

## プログラムでの使用

```rust
use libnoa::config::StorageConfig;
use libnoa::object::{create_remote_store, ObjectStore};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cfg = StorageConfig::ipfs("local", "http://127.0.0.1:5001");
    let store = create_remote_store(&cfg).await?;

    // コンテンツをリモートに保存
    let blob_id = store.put_blob(b"hello world").await?;
    println!("Stored as: {}", blob_id);

    // 存在確認
    assert!(store.has_blob(&blob_id).await?);

    // 取得
    let data = store.get_blob(&blob_id).await?;
    assert_eq!(data, b"hello world");

    Ok(())
}
```
