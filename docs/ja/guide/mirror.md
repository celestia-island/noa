# ミラーと還流

ミラーシステムは、リポジトリの master・すべてのタグ・アクティブな PR head を外部ホスト（GitHub など）から noa へ取り込みます。さらに還流レーンが noa の状態を元へ push し返します——これにより「noa が権威 + GitHub はミラー」という過渡形態が形成され、farm/CNB は GitHub から fetch し続け、変更はゼロです。

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

| フィールド | 説明 |
|---|---|
| `name` | `^[a-z0-9][a-z0-9-]{0,63}$`。スクラッチディレクトリ名・カタログのリポジトリ名・clone パスのセグメントも兼ねます |
| `remote_url` | `https://` / `file://` のみ許可。埋め込み認証情報（`@`、`?`）は拒否されます |
| `github_slug` | `owner/repo` 形状チェック。Git Data API レーン（tags / PR heads の取得）と canary プローブで使用。バックフローの push は `remote_url` を使う |
| `branches` | 最低 1 つ。`tags`/`tags/*`、`pr/*`（PR 名前空間との衝突）、派生ワークスペースの不正な形式はすべて拒否されます |
| `tags` | すべてのタグをミラーします |
| `pr_heads` | アクティブな PR head をミラーします（`refs/pull/<N>/head` → `mirror/{job}/pr/<N>`） |
| `backflow` | 各ウェーブでスクラッチの状態を元へ push します（詳細は下記） |
| `canary_push` | 書き込み方向を検証するためのプローブ参照を 1 つ push します |
| `enabled` | オフのとき、スケジューラーはそのジョブをスキップします |

CRUD: `POST/GET /api/v1/mirror/jobs`、`DELETE /api/v1/mirror/jobs/{name}`、`POST /api/v1/mirror/run`（手動トリガー、ボディは `{"job":"<name>"}`）。

## 二つのレーン

- **git レーン**: スクラッチのベアクローン + `git fetch`。タグ・PR head・還流 push はすべてこれを通ります。
- **API レーン**: git 転送が死んだ場合（ネットワークジッター系）は、自動的に Git Data API による blob 単位の取り込みへ縮退し、すべての blob の SHA を検証します（忠実度はレーンとともに低下しません）。

縮退はウェーブ単位です: `ensure_scratch` が失敗 → このウェーブは API レーンを通る → セルフヒールが壊れかけのスクラッチを削除し、次のウェーブが再クローンします。還流 push は git レーンが利用可能な間だけ実行されます（API は参照を push できません）。

## PR head

`pr_heads: true` の場合、各ウェーブはさらに `refs/pull/<N>/head` を fetch し、`mirror/{job}/pr/<N>` と同名のワークスペースとしてインポートします。PR 名前空間とブランチ名前空間（`pr/<x>` 形式のブランチ）は相互排他で、upsert 時に拒否されます。

## 還流

```text
scratch local refs ──atomic push──▶ origin
  +refs/heads/*:refs/heads/*
  +refs/tags/*:refs/tags/*
  +refs/pull-heads/*:refs/noa/pr/*     (only when pull-heads exist in the scratch)
```

- **認証情報**: `token_env`（デフォルトは `NOA_MIRROR_GH_TOKEN`）から解決されたトークンは、`x-access-token:<token>` として使い捨ての push URL に埋め込まれます。トークンがスクラッチの設定に置かれることはありません。
- **Force のセマンティクス**: 切り替え後は noa が書き込み権威となり、元の参照はスクラッチへ収束します。非対称な削除（noa 側の削除は元側の参照を削除しない）は既知の制限です。
- **失敗のセマンティクス**: 還流の失敗はそのウェーブを FAILED にします——受け入れに不可欠なレーンなので、失敗は確実に目立つように保たれます。
- **名前空間ブリッジ**: GitHub は `refs/pull/*` への push を拒否するため、PR head は `refs/noa/pr/<N>` に着地します。

## カナリアプローブ

`canary_push: true` の場合、すべてのウェーブが `noa-mirror-canary` プローブ参照（最初に複製したブランチのコミット）を元へ push します。まず git 転送を使い、失敗時は Git Data API に縮退します——書き込み方向の認証情報と接続性を継続的に検証しています。プローブの失敗はそのウェーブを目立つ形で失敗させます。

## 忠実度検証

取り込まれたすべてのコミットは項目単位で突き合わせられます: パス集合・エントリ種別・blob OID（`git_blob_oid` が git と一致）が正確に一致しなければならず、1 つでも差異があればその項目は失敗してウェーブのエラー詳細に入ります。還流はこの不変条件に依存しています：git から取り込まれた内容については、スクラッチ内の git オブジェクトと noa オブジェクトは blob 層で 1 対 1 に対応し、push に変換は不要です。**境界**：noa ネイティブに作られたコミット（純ワークスペーススナップショット）には git オブジェクト形態がなく、還流の対象外です（正確な境界は[アーキテクチャ](./architecture.md)参照）。

## 古い参照の掃除（reconcile）

タグと PR head の上流集合が再調整（reconcile）の対象です: 上流に現れなくなった参照は noa から掃除されます（`mirror/{job}/tags/` と `mirror/{job}/pr/` の名前空間）。タグの還流 push も同様に上流に従います（noa 側の削除は元側を削除しません——既知の非対称性）。

## スケジューリング

`NOA_MIRROR_ENABLED=1` のとき、スケジューラーは有効な全ジョブを 900 秒ごとに 1 周実行します。`POST /mirror/run` で手動トリガーできます。ウェーブの詳細（レーン、項目ごとの結果、backflow/カナリアの状態）は `GET /api/v1/mirror/jobs` の `last_detail` / `last_lane` に現れます。
