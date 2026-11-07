# デプロイ

noa-server のデプロイ形態：単一バイナリ + 単一 redb データベース + systemd ユニット。本番インスタンスは malkuth が管理します。

## デプロイ 3 点セット

1. **成果物**：`cargo build --release --locked --bin noa-server` の成果物をデプロイパスへコピーします（アトミック置換：先に `<bin>.new` を書いてから rename）。両側で sha256 を照合します；
2. **health リトライ**：起動後に `/api/v1/*` をポーリングします（401 = プロセスが生存し認証が機能している証拠）；
3. **ロールバック**：前バージョンの成果物パスを保持し、`mv` で戻して再起動します。

## 環境変数（A クラスのブートストラップ設定 → 0600 の env ファイル）

| 変数 | 説明 |
|---|---|
| `NOA_API_TOKEN` | ブートストラップ admin トークン。**空・空白のみは起動を拒否** |
| `NOA_MIRROR_ENABLED` | ミラースケジューリングのスイッチ |
| `NOA_MIRROR_INTERVAL_SECS` | ウェーブ間隔（デフォルト 900） |
| `NOA_MIRROR_SCRATCH_DIR` | scratch のルートディレクトリ（例：`/var/lib/noa/mirror-cache`） |
| `NOA_MIRROR_GH_TOKEN` | バックフロー / カナリアの GitHub 書き込みクレデンシャル（token_env のデフォルト名） |
| `NOA_MIRROR_GH_TOKEN_ENV` | クレデンシャルの環境変数名を上書き（デフォルトは上記） |

クレデンシャルはデプロイ面（0600 の env / systemd LoadCredential）にのみ置かれ、DB・git ツリー・ドキュメントには決して入りません。

## systemd ユニット

```ini
[Service]
EnvironmentFile=/etc/noa/noa-server.env
ExecStart=/usr/local/bin/noa-server --host 0.0.0.0 --port 3016
Restart=on-failure
```

- `0.0.0.0` でリッスンします（ファミリーノードの慣例：全経路の bearer + 書き込み可能ロールの執行により露出面は安全）；
- env 変更後は `systemctl daemon-reload && systemctl restart` を実行し、`tr '\0' '\n' < /proc/$MAINPID/environ` に新しいキーが含まれることを検証します；
- 本番インスタンスは malkuth が管理します（デプロイパスを変更する前に、まずクレームによる排他取得を行ってください）。

## ポートとドメイン

ポート割り当ては workspace 台帳（`_ledger/ports-and-domains.md`）を参照してください。本番インスタンスは 3016 で、nginx の前面がその上で TLS を終端します。

## マルチインスタンス

redb は単一ファイルデータベースです——1 インスタンス = 1 データベースファイル。マルチインスタンス = 複数ディレクトリ × 複数ポート（例：`chest-demo` と本番 chest の 2 インスタンスモード）。インスタンス間の設定移動は RPC import / export を使用し、設定ファイルをコピーしません。

## デプロイ後の検証

```bash
# 鉴权与就绪
curl -s -o /dev/null -w '%{http_code}\n' http://127.0.0.1:3016/api/v1/prs   # 期望 401
# 镜像波次
curl -X POST -H "Authorization: Bearer $T" -H "Content-Type: application/json" \
  -d '{"job":"noa"}' http://127.0.0.1:3016/api/v1/mirror/run               # 期望 202
```

ミラーの詳細とバックフローのセマンティクスは [ミラーとバックフロー](../guide/mirror.md) を参照してください。
