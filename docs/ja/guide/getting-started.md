# クイックスタート

このページでは、ゼロの状態から noa を動かすところまでを案内します: ビルド、サーバーの起動、そして最初の clone / push です。

## 前提条件

- Rust stable ≥ 1.99（`rustup` でインストール）
- git ≥ 2.39（サーバーのスマート HTTP が `upload-pack` / `receive-pack` を直接駆動します）
- 任意: node + pnpm（WebUI のビルド用）

## ビルド

```bash
git clone https://github.com/celestia-island/noa.git
cd noa
cargo build --release --locked
```

成果物: `target/release/noa`（CLI）と `target/release/noa-server`（サーバー）。

## サーバーの起動

最小構成に必要なのは API トークン 1 つ（ブートストラップ管理者の認証情報）だけです:

```bash
export NOA_API_TOKEN="<your-token>"
export NOA_MIRROR_SCRATCH_DIR=/var/lib/noa/mirror-cache   # mirror scratch root
export NOA_MIRROR_ENABLED=1                               # enable mirror scheduling (default 900s)
export NOA_MIRROR_GH_TOKEN="<github-pat>"                 # optional: back-flow / canary push credential
noa-server --host 0.0.0.0 --port 3016
```

起動したら:

```bash
curl -H "Authorization: Bearer <your-token>" http://127.0.0.1:3016/api/v1/repos
```

`[]` が返ればサービスの準備完了です。すべての REST エンドポイントは Bearer 認証を要求します。`/git/{repo}/*` 配下の読み取り系パスも同様です（認証のないリクエストは `Basic` チャレンジ付きの 401 を受け取ります）。

## noa ネイティブワークフロー（推奨）

`noa` CLI は第一級の入口です——clone してくるのは `.git` と `.noa` が共存するリポジトリです（git がソースを、noa がエージェントデータを管理します）:

```bash
noa clone http://<token>@127.0.0.1:3016/git/hello --path hello
cd hello
noa workspace create agent-a     # a per-agent isolated workspace
# … edit files …
noa snapshot create -m "change" -a agent-a
noa log                          # this workspace's snapshot history
noa push                         # publishes the workspace snapshot (serialized to a git commit)
```

`noa push` が公開するのは**ワークスペースのスナップショット状態**（`[noa export]` コミットを生成）であり、`.git` のコミットではありません。2 つのレーンは並行で、混在させないでください（push の前に `noa snapshot create` を実行しないと、古いスナップショットが公開されます）。認証は `NOA_GIT_USER` + `NOA_GIT_PASS` 環境変数から注入されます。ディレクトリ全体（`.git/noa` を含む）を別のマシンへコピーしても、そのまま動き続けます。

## git 互換レーン（どの git ツールでも使える）

同じ `/git/{repo}` エンドポイントは標準の git スマート HTTP も話します——素の git、CI、farm もそのまま使えます:

```bash
git clone http://<token>@127.0.0.1:3016/git/hello
```

push（receive-pack）には書き込みロールが必要です。着地した push は**即座にインポートウェーブをトリガーします**（還流は同じウェーブで続きます）。これが farm/CNB をゼロ変更に保つ過渡形態です。

## 最初のミラー（GitHub リポジトリを noa に取り込む）

```bash
curl -X POST -H "Authorization: Bearer <your-token>" \
  -H "Content-Type: application/json" \
  -d '{
    "name": "hello",
    "remote_url": "https://github.com/you/hello.git",
    "github_slug": "you/hello",
    "branches": ["master"],
    "tags": true,
    "pr_heads": true,
    "enabled": true
  }' \
  http://127.0.0.1:3016/api/v1/mirror/jobs

curl -X POST -H "Authorization: Bearer <your-token>" \
  -H "Content-Type: application/json" \
  -d '{"job":"hello"}' \
  http://127.0.0.1:3016/api/v1/mirror/run
```

ウェーブが完了すれば、そのリポジトリは noa 経由で clone できるようになります:

```bash
git clone http://<your-token>@127.0.0.1:3016/git/hello
```

## バックアップと移行

サーバーの状態全体を、持ち運び可能な 1 つのディレクトリへエクスポートできます（詳細は[デプロイ](../reference/deploy.md)のバックアップ節）: redb の一貫性スナップショット + リポジトリごとに 1 つの git バンドル + sha256 マニフェスト。`noa-server --import-state <dir>` でオフライン復元します。受け入れの基準はリハーサルです: エクスポート → 別マシンで復元 → 起動して照合。

## 次のステップ

- [アーキテクチャ概観](./architecture.md) — ワークスペース / スナップショット / 参照モデル
- [ミラーと還流](./mirror.md) — 二つのレーン、PR head、還流とカナリア
- [HTTP API リファレンス](../reference/http-api.md) — すべてのエンドポイント
