# 權限與用戶

noa 的鑑權是「單 token、雙通道」：REST 與 git 共用同一套憑證，但寫權限各自執法。

## 憑證

- **引導 token**：`NOA_API_TOKEN` 環境變數。伺服器端啟动强制非空（空/全空白拒绝啟动），
  该 token 即 admin 身份。
- **注册用戶 token**：经 REST 建立（见下），sha256 哈希存储，常數时间比较。

认證形態（两通道一致）：

```text
Authorization: Bearer <token>
Authorization: Basic <base64(any-user:token)>     # git 用戶端惯例：用戶名任意
```

空 token 伺服器端（`NOA_API_TOKEN=""`）不会退化为匿名可讀——啟动即拒绝，且比较前的
非空守卫雙重防御。

## 角色

| 角色 | 讀 | 寫（REST / git push） | 管理（用戶 CRUD） |
|---|---|---|---|
| admin | ✓ | ✓ | ✓ |
| operator | ✓ | ✓ | ✗ |
| reader | ✓ | ✗ | ✗ |

- REST：中间件集中執法——reader 的寫请求 403；`/api/v1/users` 非 GET 403（仅 admin）。
- git：push（receive-pack）要求 can_write——无憑證 401（带 Basic challenge，git 用戶端
  據此提示）；有憑證但 reader 403（提示需要 write 角色）。clone/fetch 任意已认證身份。

## 用戶管理 REST

```bash
# 建立（admin）
curl -X POST -H "Authorization: Bearer $ADMIN" -H "Content-Type: application/json" \
  -d '{"name":"alice","role":"operator","token":"<alice-token>"}' \
  http://127.0.0.1:3016/api/v1/users

# 列表（admin）／單个
curl -H "Authorization: Bearer $ADMIN" http://127.0.0.1:3016/api/v1/users
curl -H "Authorization: Bearer $ADMIN" http://127.0.0.1:3016/api/v1/users/alice

# 刪除（admin）
curl -X DELETE -H "Authorization: Bearer $ADMIN" \
  http://127.0.0.1:3016/api/v1/users/alice
```

規則：

- `name` 與 `role`（admin/operator/reader）必填；token 伺服器端存储前做 sha256。
- 更新（同端點 POST 同名）可輪换 token 或改角色；`created_us` 保留原值。
- 刪除是 admin 专屬且不可逆。

## git 通道示例

```bash
# 讀（任意已认證身份；公開仓用占位用戶名即可）
git clone http://<token>@noa.example.com:3016/git/noa

# 寫（需要 operator/admin）
git push origin master
```

未认證 push 收到 401 + Basic challenge（git 用戶端據此提示憑證）；reader 身份 push 收到
403（提示需要 write 角色）。
