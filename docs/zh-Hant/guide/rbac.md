# 權限與使用者

noa 的驗證是「單一 token、雙通道」：REST 與 git 共用同一套憑證，但寫入權限各自強制執行。

## 憑證

- **引導 token**：`NOA_API_TOKEN` 環境變數。伺服器啟動時強制非空（空值/全空白拒絕啟動），
  該 token 即 admin 身分。
- **註冊使用者 token**：經 REST 建立（見下），以 sha256 雜湊儲存，常數時間比較。

驗證形態（兩通道一致）：

```text
Authorization: Bearer <token>
Authorization: Basic <base64(any-user:token)>     # git 用戶端慣例：使用者名稱任意
```

空 token 的伺服器（`NOA_API_TOKEN=""`）不會退化為匿名可讀——啟動即拒絕，且比較前的
非空防護檢查提供雙重防禦。

## 角色

| 角色 | 讀 | 寫（REST / git push） | 管理（使用者 CRUD） |
|---|---|---|---|
| admin | ✓ | ✓ | ✓ |
| operator | ✓ | ✓ | ✗ |
| reader | ✓ | ✗ | ✗ |

- REST：由中介層集中強制執行——reader 的寫入請求回 403；`/api/v1/users` 非 GET 回 403（僅 admin）。
- git：push（receive-pack）要求 can_write——無憑證回 401（帶 Basic challenge，git 用戶端
  據此提示）；有憑證但身分為 reader 回 403（提示需要 write 角色）。clone/fetch 允許任意已驗證身分。

## 使用者管理 REST

```bash
# 建立（admin）
curl -X POST -H "Authorization: Bearer $ADMIN" -H "Content-Type: application/json" \
  -d '{"name":"alice","role":"operator","token":"<alice-token>"}' \
  http://127.0.0.1:3016/api/v1/users

# 列表（admin）／單一
curl -H "Authorization: Bearer $ADMIN" http://127.0.0.1:3016/api/v1/users
curl -H "Authorization: Bearer $ADMIN" http://127.0.0.1:3016/api/v1/users/alice

# 刪除（admin）
curl -X DELETE -H "Authorization: Bearer $ADMIN" \
  http://127.0.0.1:3016/api/v1/users/alice
```

規則：

- `name` 與 `role`（admin/operator/reader）必填；token 在伺服器端儲存前先做 sha256。
- 更新（同一端點 POST 同名）可輪替 token 或更改角色；`created_us` 保留原值。
- 刪除是 admin 專屬且不可逆。

## git 通道範例

```bash
# 讀（任意已驗證身分；公開儲存庫用佔位使用者名稱即可）
git clone http://<token>@noa.example.com:3016/git/noa

# 寫（需要 operator/admin）
git push origin master
```

未驗證的 push 收到 401 + Basic challenge（git 用戶端據此提示輸入憑證）；reader 身分的 push 收到
403（提示需要 write 角色）。
