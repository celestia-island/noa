# git-http プロトコル

`/git/{repo}/*` はスマート HTTP プロトコルを提供します: 読み取り系は `git upload-pack --stateless-rpc` を直接駆動し、書き込み系（W2a2 以降）は `git receive-pack --stateless-rpc` を直接駆動します——`git-http-backend` CGI ラッパーを使わないため、プロトコルネゴシエーション・環境変数・エラーの形状を完全に制御できます。

## エンドポイント

| メソッド | パス | 目的 |
|---|---|---|
| GET | `/git/{repo}/info/refs?service=git-upload-pack` | clone/fetch のアドバタイズ |
| POST | `/git/{repo}/git-upload-pack` | パックのネゴシエーション |
| GET | `/git/{repo}/info/refs?service=git-receive-pack` | push のアドバタイズ |
| POST | `/git/{repo}/git-receive-pack` | push の受け入れ |

`{repo}` はミラージョブ名でなければなりません——提供本体はそのジョブのスクラッチベアクローン（`NOA_MIRROR_SCRATCH_DIR/{repo}.git`）です。存在しないリポジトリは 404 を返します。

## 動作の詳細

- **プロトコルネゴシエーション**: クライアントの `Git-Protocol` ヘッダーは子プロセスへそのまま渡されます——ここで言っているのは **git ワイヤープロトコルの上流側バージョン**です（gitprotocol-v2。git 自身の規格であり、noa のバージョン番号ではありません）。空ならデフォルトの v0 です。これこそ、直接駆動の実装が git-http-backend を置き換えた理由です——CGI ラッパーは POST 経路でこのヘッダーを落とし、黙って v0 にダウングレードして clone を壊します。
- **アドバタイズのプレアンブル**: `# service=` pkt-line（サービス名から長さヘッダーを動的に計算）+ flush。RPC レスポンスにプレアンブルは付きません。
- **Content-Type**: `application/x-git-{upload,receive}-pack[-advertisement]` を厳密に区別します。
- **リクエストボディ**: POST ボディは子プロセスの stdin にそのまま書き込まれます。`Content-Encoding: gzip` は自動展開されます（展開比 > 1000:1 は拒否——bomb 対策。リクエスト 50 MiB / 展開後 200 MiB の上限）。
- **タイムアウト**: 交換全体に 60 秒のウォールクロックがあり、タイムアウトで子プロセスを kill します（kill_on_drop が孤児のロック保持者を防ぎます）。
- **並行性**: ルートレベルのセマフォが進行中の git 交換数を制限します（メモリ上限 4×50 MiB）。
- **子プロセスの環境**: `env_clear` の後は `PATH` + `GIT_PROTOCOL` しか残りません——環境変数の漏えいは構造上起こり得ません。

## push のセマンティクス

push はスクラッチのローカル参照に着地します。**receive-pack が成功した時点で、そのジョブのインポートウェーブが即座にトリガーされます**（スケジューラを待ちません）——push された内容は直ちに API 面（参照 / スナップショット / ワークスペース）に現れます。ウェーブはその後:

1. スクラッチが進めた参照を noa の参照システム（スナップショット / ワークスペース）へ取り込みます
2. `backflow` が有効なとき、その状態を元へ push し返します

したがって push には特別なフックが不要です——3 つのレーン（読み取り、receive、還流）が 1 つのスクラッチを共有し、閉じたループが自動的に収束します。push には認証済みのアイデンティティが必要です。認証情報がなければ 401 + `Basic` チャレンジ、reader ロールは 403 です。

## 既知の境界

- push には削除のセマンティクスがありません（noa 側の削除は元へ伝播しません——既知の非対称性）。
- ダム HTTP（素の `/info/refs` ファイル）は提供しません。
- PR head は還流時に `refs/noa/pr/<N>` に着地します（GitHub は `refs/pull/*` への push を拒否します）。
- バックフロー push のソースはスクラッチの git オブジェクトです——**noa ネイティブに作られたコミット**（スクラッチを経由しない純ワークスペーススナップショット）は、スナップショット→git オブジェクトのシリアライザが実装されるまで還流の対象外です（[アーキテクチャ](./architecture.md)参照）。
