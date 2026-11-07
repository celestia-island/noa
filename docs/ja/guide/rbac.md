# RBAC

noa の認証は「1 つのトークン、2 つのチャネル」です: REST と git は同じ認証情報を共有しますが、書き込み権限はそれぞれのチャネルで独立に強制されます。

## 認証情報

- **ブートストラップトークン**: `NOA_API_TOKEN` 環境変数です。サーバーは空・空白の値での起動を拒否します。このトークンこそが管理者アイデンティティです。
- **登録ユーザーのトークン**: REST 経由（下記）で作成され、sha256 ハッシュで保存され、定数時間で比較されます。

認証形式（両チャネルで同一）:

```text
Authorization: Bearer <token>
Authorization: Basic <base64(any-user:token)>     # git client convention: any username
```

`NOA_API_TOKEN=""` で起動したサーバーが匿名読み取りへフォールバックすることは決してありません——起動時に拒否され、比較前の非空ガードが第二の防御層になります。

## ロール

| ロール | 読み取り | 書き込み（REST / git push） | 管理者（ユーザー CRUD） |
|---|---|---|---|
| admin | ✓ | ✓ | ✓ |
| operator | ✓ | ✓ | ✗ |
| reader | ✓ | ✗ | ✗ |

- REST: ミドルウェアが中央で強制します——reader の書き込みは 403。`/api/v1/users` への非 GET は 403（admin のみ）。
- git: push（receive-pack）は can_write を要求します——認証情報がなければ `Basic` チャレンジ付きの 401（git クライアントはこれでプロンプトを出します）、reader のアイデンティティは 403（エラーは書き込みロール名を示します）。clone/fetch は任意の認証済みアイデンティティで動作します。

## ユーザー管理 REST

```bash
# create (admin)
curl -X POST -H "Authorization: Bearer $ADMIN" -H "Content-Type: application/json" \
  -d '{"name":"alice","role":"operator","token":"<alice-token>"}' \
  http://127.0.0.1:3016/api/v1/users

# list (admin) / single
curl -H "Authorization: Bearer $ADMIN" http://127.0.0.1:3016/api/v1/users
curl -H "Authorization: Bearer $ADMIN" http://127.0.0.1:3016/api/v1/users/alice

# delete (admin)
curl -X DELETE -H "Authorization: Bearer $ADMIN" \
  http://127.0.0.1:3016/api/v1/users/alice
```

ルール:

- `name` と `role`（admin/operator/reader）は必須です。トークンは sha256 ハッシュで保存されます。
- 更新（同名での POST）はトークンのローテートまたはロールの変更を行い、`created_us` は保持されます。
- 削除は admin のみで、不可逆です。

## git チャネルの例

```bash
# read (any authenticated identity; public repos use a placeholder username)
git clone http://<token>@noa.example.com:3016/git/noa

# write (operator/admin required)
git push origin master
```

認証のない push は 401 + Basic チャレンジを受け取り、reader のアイデンティティは書き込みロール名を示す 403 を受け取ります。
