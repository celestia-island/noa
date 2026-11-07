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

## 最初の push（書き込み権威）

push は `/git/{repo}` 配下の receive-pack 経路を通り、認証済みのアイデンティティを必要とします:

```bash
cd hello
echo "hello noa" > README.md
git add -A && git commit -m "hello noa"
git push origin master
```

push はミラーのスクラッチに着地し、次のミラーウェーブがそれを自動的に noa のスナップショット/参照システムへ取り込みます。ジョブで `backflow` が有効な場合は、その状態を元（GitHub）へも push し返します——これが「noa が権威 + GitHub はミラー」という過渡形態です。

## 次のステップ

- [アーキテクチャ概観](./architecture.md) — ワークスペース / スナップショット / 参照モデル
- [ミラーと還流](./mirror.md) — 二つのレーン、PR head、還流とカナリア
- [HTTP API リファレンス](../reference/http-api.md) — すべてのエンドポイント
