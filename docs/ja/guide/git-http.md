# git-http プロトコル

`/git/{repo}/*` はスマート HTTP プロトコルを提供します: 読み取りは `git upload-pack --stateless-rpc` を直接駆動し、書き込み（W2a2 以降）は `git receive-pack --stateless-rpc` を駆動します——`git-http-backend` CGI ラッパーを使わないため、プロトコルネゴシエーション・環境・エラーの形状を完全に制御できます。

## エンドポイント

| メソッド | パス | 目的 |
|---|---|---|
| GET | `/git/{repo}/info/refs?service=git-upload-pack` | clone/fetch のアドバタイズ |
| POST | `/git/{repo}/git-upload-pack` | パックのネゴシエーション |
| GET | `/git/{repo}/info/refs?service=git-receive-pack` | push のアドバタイズ |
| POST | `/git/{repo}/git-receive-pack` | push の受け入れ |

`{repo}` はミラージョブ名でなければなりません——提供本体はそのジョブのスクラッチベアクローン（`NOA_MIRROR_SCRATCH_DIR/{repo}.git`）です。存在しないリポジトリは 404 を返します。

## 動作の詳細

- **プロトコルネゴシエーション**: クライアントの `Git-Protocol` ヘッダーは子プロセスへ転送されます（要求があれば v2、空ならデフォルトの v0）。これこそ、直接駆動の実装が git-http-backend を置き換えた理由です——CGI ラッパーは POST 経路でこのヘッダーを落とし、黙って v0 にダウングレードして clone を壊します。
- **アドバタイズのプレアンブル**: `# service=` pkt-line（サービス名から長さヘッダーを計算）+ flush。RPC レスポンスにはプレアンブルが付きません。
- **Content-Type**: `application/x-git-{upload,receive}-pack[-advertisement]` を厳密に区別します。
- **リクエストボディ**: POST ボディは子プロセスの stdin に書き込まれます。`Content-Encoding: gzip` は自動展開されます（圧縮比 > 1000:1 は拒否——bomb 対策。リクエスト 50 MiB / 展開後 200 MiB の上限）。
- **タイムアウト**: 交換全体に 60 秒のウォールクロックがあり、タイムアウトで子プロセスを kill します（kill_on_drop が孤児のロック保持者を防ぎます）。
- **並行性**: ルートレベルのセマフォが進行中の git 交換数を制限します（メモリは 4×50 MiB で上限化）。
- **子プロセスの環境**: `env_clear` により `PATH` と `GIT_PROTOCOL` だけを残します——環境変数の漏えいは構造上不可能です。

## push のセマンティクス

push はスクラッチのローカル参照に着地し、その後ミラーウェーブが:

1. 進んだスクラッチ参照を noa の参照システム（スナップショット / ワークスペース）へ取り込みます
2. `backflow` が有効なら、その状態を元へ push し返します

特別なフックは不要です——3 つのレーン（読み取り、receive、還流）が 1 つのスクラッチを共有し、ループは自分で収束します。push には認証済みのアイデンティティが必要です。認証情報がなければ 401 + Basic チャレンジ、reader ロールは 403 です。

## 既知の境界

- push には削除のセマンティクスがありません（noa 側の削除は元へ伝播しません——既知の非対称性）。
- ダム HTTP（素の `/info/refs` ファイル）は提供しません。
- PR head の還流は `refs/noa/pr/<N>` に着地します（GitHub は `refs/pull/*` への push を拒否します）。
