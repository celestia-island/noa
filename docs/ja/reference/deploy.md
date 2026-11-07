# デプロイ

noa-server のデプロイ形態: 単一バイナリ + 単一 redb データベース + 1 つの systemd ユニット。本番インスタンスは malkuth が監督します。

## デプロイの三点セット

1. **成果物**: `cargo build --release --locked --bin noa-server` の出力をデプロイパスへコピーします（アトミック置換: まず `<bin>.new` に書いてから rename）。両側で sha256 を検証します
2. **health リトライ**: 起動後に `/api/v1/*` をポーリングします（401 = プロセスが生きていて認証が効いている証拠です）
3. **ロールバック**: 前の成果物パスを残しておき、`mv` で戻して再起動します

## 環境変数（A 類ブートストラップ設定 → 0600 の env ファイル）

| 変数 | 説明 |
|---|---|
| `NOA_API_TOKEN` | ブートストラップ管理者トークン。**空・空白のみは起動を拒否します** |
| `NOA_MIRROR_ENABLED` | ミラースケジューリングのスイッチ |
| `NOA_MIRROR_INTERVAL_SECS` | ウェーブ間隔（デフォルト 900） |
| `NOA_MIRROR_SCRATCH_DIR` | スクラッチのルート（例: `/var/lib/noa/mirror-cache`） |
| `NOA_MIRROR_GH_TOKEN` | 還流 / カナリア用の GitHub 書き込み認証情報（デフォルトの `token_env` 名） |
| `NOA_MIRROR_GH_TOKEN_ENV` | 認証情報を保持する環境変数名を上書きします（デフォルトは上の行） |

認証情報はデプロイ面（0600 の env / systemd LoadCredential）にだけ置かれます。DB・git ツリー・文書に置くことは決してありません。

## systemd ユニット

```ini
[Service]
EnvironmentFile=/etc/noa/noa-server.env
ExecStart=/usr/local/bin/noa-server --host 0.0.0.0 --port 3016
Restart=on-failure
```

- `0.0.0.0` でリッスンします（family-node の慣習: 全ルートの Bearer 認証 + 書き込みロールの強制により、安全な露出面になります）
- env 変更後: `systemctl daemon-reload && systemctl restart`。`tr '\0' '\n' < /proc/$MAINPID/environ` に新しいキーが含まれることを確認します
- 本番インスタンスは malkuth の監督下にあります（デプロイパスに手を入れる前にデプロイ独占の claim を取得してください）

## ポートとドメイン

ポートの割り当てはワークスペース台帳（`_ledger/ports-and-domains.md`）にあります。本番は 3016 で、その上に nginx のフロントドアが TLS 終端を行います。

## 複数インスタンス

redb は単一ファイルストアです——1 インスタンスに 1 つのデータベースファイル。複数インスタンス = 複数ディレクトリ + 複数ポート（例: `chest-demo` と本番 chest の二重インスタンスモード）。インスタンス間の設定移送は RPC import / export を通します。設定ファイルのコピーは決して行いません。

## バックアップと復元

サーバーの状態全体 = redb ファイル + スクラッチディレクトリ + env です（認証情報は**意図的に除外されます**——認証情報の復元は再設定であって、コピーではありません）。

```bash
# export (admin; live is fine — the write lock makes the copy consistent)
curl -X POST -H "Authorization: Bearer $ADMIN" -H "Content-Type: application/json" \
  -d '{"out": "/srv/noa-backup/nightly"}' \
  http://127.0.0.1:3016/api/v1/admin/export

# artifacts: noa.redb (consistent snapshot) + {job}.bundle (cloneable per-repo
# file) + manifest.json (sha256)

# restore (offline — restore before the database ever opens)
noa-server --db-path /new/noa.redb --import-state /srv/noa-backup/nightly --port 3016
```

復元はまずすべての成果物の sha256 を検証します（半分壊れたバックアップは 1 ファイルも書き込みません）。既存のスクラッチは保持されます（進んでいる可能性があり、次のウェーブで照合されます）。**受け入れの基準はリハーサルです**: エクスポート → 別マシンで復元 → 起動して照合。定期エクスポートには systemd timer を推奨します。

## デプロイ後の検証

```bash
# Auth and readiness
curl -s -o /dev/null -w '%{http_code}\n' http://127.0.0.1:3016/api/v1/prs   # expect 401
# Mirror wave
curl -X POST -H "Authorization: Bearer $T" -H "Content-Type: application/json" \
  -d '{"job":"noa"}' http://127.0.0.1:3016/api/v1/mirror/run               # expect 202
```

ミラーの詳細と還流のセマンティクス: [ミラーと還流](../guide/mirror.md) を参照。
