# ミラーと還流

ミラーシステムは、リポジトリの master・すべてのタグ・アクティブな PR head を外部リモート（GitHub など）から noa へ取り込みます。還流レーンは noa の状態を元へ push し返します——これが「noa が権威 + GitHub はミラー」という移行形態で、farm/CNB はGitHub から fetch し続け、変更はゼロです。

## ミラージョブ（MirrorJob）

```json
{
  "name": "noa",
  "remote_url": "https://github.com/celestia-island/noa.git",
  "github_slug": "celestia-island/noa",
  "branches": ["master"],
  "tags": true,
  "pr_heads": true,
  "backflow": true,
  "canary_push": true,
  "enabled": true
}
```

| フィールド | 意味 |
|---|---|
| `name` | `^[a-z0-9][a-z0-9-]{0,63}$`。スクラッチディレクトリ名・カタログのリポジトリ名・clone パスのセグメントも兼ねます |
| `remote_url` | `https://` / `file://` のみ。埋め込み認証情報（`@`、`?`）は拒否されます |
| `github_slug` | `owner/repo` 形式のチェック。Git Data API レーンと還流で使用します |
| `branches` | 最低 1 つ。`tags`/`tags/*` と `pr/*`（PR 名前空間との衝突）の形式は拒否されます |
| `tags` | すべてのタグをミラーします |
| `pr_heads` | アクティブな PR head をミラーします（`refs/pull/<N>/head` → `mirror/{job}/pr/<N>`） |
| `backflow` | 各ウェーブでスクラッチの状態を元へ push します（下記参照） |
| `canary_push` | 書き込み方向を検証するプローブ参照を push します |
| `enabled` | 無効化されたジョブはスケジューラーがスキップします |

CRUD: `POST/GET /api/v1/mirror/jobs`、`DELETE /api/v1/mirror/jobs/{name}`、`POST /api/v1/mirror/run`（手動トリガー、ボディは `{"job":"<name>"}`）。

## 二つのレーン

- **git レーン**: スクラッチのベアクローン + `git fetch`。タグ・PR head・還流 push はすべてこれを使います。
- **API レーン**: git 転送が死んだ場合（ネットワークフラップ系）は、ウェーブが blob 単位の Git Data API 取り込みに縮退し、各 blob の SHA を検証します（忠実度はレーンとともに低下しません）。

縮退はウェーブ単位です: `ensure_scratch` の失敗 → このウェーブは API レーンで実行 → セルフヒールが壊れかけのスクラッチを削除し、次のウェーブが再クローンします。還流 push は git レーンが生きているときにだけ実行されます（API は参照を push できません）。

## PR head

`pr_heads: true` の場合、各ウェーブはさらに `refs/pull/<N>/head` を取り込み、`mirror/{job}/pr/<N>` と同名のワークスペースとしてインポートします。PR 名前空間とブランチ名前空間（`pr/<x>` 形式のブランチ）は相互排他で、upsert は衝突を拒否します。

## 還流

```text
scratch local refs ──atomic push──▶ origin
  +refs/heads/*:refs/heads/*
  +refs/tags/*:refs/tags/*
  +refs/pull-heads/*:refs/noa/pr/*     (only when pull-heads exist)
```

- **認証情報**: `token_env`（デフォルトは `NOA_MIRROR_GH_TOKEN`）から解決されたトークンは、`x-access-token:<token>` 形式で使い捨ての push URL に埋め込まれます。トークンがスクラッチの設定に置かれることはありません。
- **Force のセマンティクス**: 切り替え後は noa が書き込み権威です——元の参照はスクラッチの状態へ収束します。削除の非対称性（noa 側の削除は元側の参照を削除しない）は既知の制限です。
- **失敗のセマンティクス**: 還流の失敗はそのウェーブを FAILED にします——受け入れに不可欠なレーンなので、失敗は確実に目立つように報告されます。
- **名前空間ブリッジ**: GitHub は `refs/pull/*` への push を拒否するため、PR head は `refs/noa/pr/<N>` に着地します。

## カナリアプローブ

`canary_push: true` の場合、各ウェーブは `noa-mirror-canary` プローブ参照（最初に複製したブランチのコミット）を元へ push します——まず git 転送で、失敗時は Git Data API に縮退——し、書き込み方向の認証情報と接続性を継続的に検証します。カナリアの失敗はそのウェーブを目立つ形で失敗させます。

## 忠実度検証

取り込まれたすべてのコミットはエントリ単位で突き合わせられます: パス集合・エントリ種別・blob OID（`git_blob_oid` が git と一致）が正確に一致しなければならず、1 つでも差異があればその項目はウェーブのエラー詳細に入ります。還流はこの不変条件に依存しています——noa のオブジェクトは git オブジェクトに 1 対 1 で対応するため、push に変換レイヤーは不要です。

## 古い参照の再調整

タグと PR head の上流集合が再調整（reconcile）の対象です: 上流に存在しなくなった参照は noa から掃除されます（`mirror/{job}/tags/` と `mirror/{job}/pr/` の名前空間）。タグの還流 push も同様に上流駆動です（noa 側の削除は元側の参照を削除しません——既知の非対称性）。

## スケジューリング

`NOA_MIRROR_ENABLED=1` のとき、スケジューラーは有効な全ジョブを 900 秒ごとに実行します。`POST /mirror/run` で手動トリガーできます。ウェーブの詳細（レーン、項目ごとの結果、還流/カナリアの状態）は `GET /api/v1/mirror/jobs` の `last_detail` / `last_lane` に現れます。
