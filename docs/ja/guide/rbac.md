# 権限とユーザー

noa の認証は「1 つのトークン、2 つのチャネル」です: REST と git は同じ認証情報を共有しますが、書き込み権限はそれぞれのチャネルで独立に強制されます。

## 認証情報

- **ブートストラップトークン**: `NOA_API_TOKEN` 環境変数です。空の値（空文字列・空白のみ）は起動時に拒否されます。このトークンこそが管理者アイデンティティです。
- **登録ユーザーのトークン**: REST 経由（下記参照）で作成され、sha256 ハッシュで保存され、定数時間で比較されます。

認証形式（両チャネルで同一）:

```text
Authorization: Bearer <token>
Authorization: Basic <base64(any-user:token)>     # git client convention: any username
```

空トークン（`NOA_API_TOKEN=""`）のサーバーが匿名読み取りへ縮退することはありません——起動がその場で拒否され、比較前の非空ガードが多層防御を追加します。

## ロール

| ロール | 読み取り | 書き込み（REST / git push） | 管理者（ユーザー CRUD） |
|---|---|---|---|
| admin | ✓ | ✓ | ✓ |
| operator | ✓ | ✓ | ✗ |
| reader | ✓ | ✗ | ✗ |

- REST: 強制はミドルウェアに一元化されています——reader の書き込みリクエストは 403。`/api/v1/users` への非 GET は 403（admin のみ）。
- git: push（receive-pack）は can_write を要求します——認証情報がなければ 401（`Basic` チャレンジ付き。git クライアントはこれでプロンプトを出します）、認証情報はあるが reader なら 403（書き込みロールが必要であることを通知します）。clone/fetch は任意の認証済みアイデンティティで動作します。

## ユーザー管理 REST

```bash
# Create (admin)
curl -X POST -H "Authorization: Bearer $ADMIN" -H "Content-Type: application/json" \
  -d '{"name":"alice","role":"operator","token":"<alice-token>"}' \
  http://127.0.0.1:3016/api/v1/users

# List (admin) / single
curl -H "Authorization: Bearer $ADMIN" http://127.0.0.1:3016/api/v1/users
curl -H "Authorization: Bearer $ADMIN" http://127.0.0.1:3016/api/v1/users/alice

# Delete (admin)
curl -X DELETE -H "Authorization: Bearer $ADMIN" \
  http://127.0.0.1:3016/api/v1/users/alice
```

ルール:

- `name` と `role`（admin/operator/reader）は必須です。サーバーはトークンを sha256 ハッシュ化してから保存します。
- 更新（同じエンドポイントへの同名での POST）はトークンのローテートまたはロールの変更ができ、`created_us` は元の値を保持します。
- 削除は admin のみで、不可逆です。

## git チャネルの例

```bash
# Read (any authenticated identity; public repos work with a placeholder username)
git clone http://<token>@noa.example.com:3016/git/noa

# Write (requires operator/admin)
git push origin master
```

認証のない push は 401 + `Basic` チャレンジを受け取り（git クライアントはこれで認証情報を尋ねます）、reader のアイデンティティによる push は 403 を受け取ります（書き込みロールが必要であることが通知されます）。
