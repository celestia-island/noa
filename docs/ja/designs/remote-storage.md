# リモートストレージバックエンドの設計

## 概要

noaは、コンテンツアドレス型オブジェクトの配布とバックアップのためのプラグ可能なリモートストレージバックエンドをサポートしています。すべてのバックエンドは同じ `ObjectStore` トレイトを実装しているため、スナップショット、ツリー、blobは設定された任意のバックエンドに相互にプッシュできます。

## サポートされているバックエンド

| バックエンド | 型識別子 | トランスポート | 分配モデル |
|---------|----------------|-----------|-------------------|
| Redb (ローカル) | — (常にローカル) | 組み込みKV | なし |
| IPFS (Kubo) | `ipfs` | HTTP API | ピアツーピア (DHT, Bitswap) |
| S3 / MinIO | `s3` | S3互換API | 中央集権型オブジェクトストア |

## 設定

リモートバックエンドは `.noa/config` の `[[storage]]` 配列として保存されます:

```toml
[[storage]]
name = "ipfs-local"
type = "ipfs"
endpoint = "http://127.0.0.1:5001"
gateway = "https://ipfs.io"
auto_pin = false

[[storage]]
name = "s3-backup"
type = "s3"
endpoint = "https://s3.example.com"
bucket = "noa-objects"
access_key = "AKIA..."
secret_key = "..."
region = "us-east-1"
```

各エントリは `name` (CLI参照用)、`type` 識別子、およびバックエンド固有のフィールドを持ちます。特定の型に対する未知のフィールドは無視されます。

## ファクトリパターン

`create_remote_store(&StorageConfig) -> Box<dyn ObjectStore>` は `backend_type` フィールドを検査し、適切な実装を構築します:

```
type = "ipfs"  →  IpfsObjectStore  (reqwest HTTPクライアント → Kuboデーモン)
type = "s3"    →  MinioObjectStore (aws-sdk-s3 → S3互換エンドポイント)
```

## IPFS CIDブリッジ

noaはオブジェクトを16進数エンコードされたSHA-256ハッシュ (`BlobId`, `TreeId`) で識別します。IPFSでは、これらはAPI呼び出しのためにCIDv1 (rawコーデック) に変換されます:

```
CIDv1 bytes = [0x01]           // バージョン1
              [0x55]           // rawコーデック
              [0x12]           // sha2-256ハッシュ関数
              [0x20]           // 32バイトのダイジェスト長
              [32 bytes hash]

CIDv1 string = "b" + base32_lowercase_nopad(CIDv1 bytes)
```

この変換は純粋関数です — 同じコンテンツは常に同じCIDにマッピングされます。このマッピングにデーモンのラウンドトリップは不要です。

## ライブラリの選択: ipfs-api-backend-hyperではなくreqwest

IPFSバックエンドは `ipfs-api-backend-hyper` クレートではなく、Kubo HTTP APIに対して `reqwest` を直接使用します。理由:

- `aws-sdk-s3` (既に依存関係にある) は内部でhyperを使用しており、`ipfs-api-backend-hyper` を追加するとhyperのバージョン競合のリスクがある
- Kubo APIはシンプルなREST呼び出しに十分シンプルである
- `rustls-tls` 付きの `reqwest` はOpenSSLのシステム依存を回避する

## プッシュ戦略

スナップショットをプッシュする際、noaはツリーを再帰的に走査します:

1. ツリー内の各blobについて: リモートに存在するか確認 → 存在しない場合、プッシュする
2. 各サブツリーについて: 再帰する
3. ルートツリーをプッシュする
4. `--pin` 付きのIPFSの場合: ガベージコレクションを防ぐためにルートCIDをピン留めする

これにより、スナップショットグラフ全体が転送されます。ローカルの `RedbObjectStore` が常に信頼できるソースであり、リモートバックエンドは分配/バックアップのターゲットです。

## エラーハンドリング

バックエンド固有のエラーは `NoaError` のバリアントにマッピングされます:

- `IpfsDaemonUnreachable { endpoint }` — 接続拒否、タイムアウト
- `IpfsError { message }` — APIエラーレスポンス
- `InvalidCid { cid }` — SHA-256 → CID変換の失敗
- `ObjectNotFound { id }` — ネットワーク/ストアでブロックが見つからない

## 設計判断

### なぜタグ付きenumではなくフラットな設定構造体なのか?

TOMLにはネイティブのenumサポートがありません。`type` 識別子フィールドとオプションのバックエンド固有フィールドを持つフラットな構造体は、最もTOMLに適したアプローチであり、既存の `RemoteConfig` パターン (`name` + `url` + `protocol`) と一致します。

### なぜ `[[remotes]]` と統合しないのか?

Gitリモート (`RemoteConfig`) とオブジェクトストレージ (`StorageConfig`) は異なる目的を持ちます:
- **リモート** はgitプロトコルのプッシュ/プル用 (ソースコードの配布)
- **ストレージ** はコンテンツアドレス型オブジェクトの配布用 (スナップショット、blob)

これらを分けておくことで混乱を避け、独立した設定が可能になります。
