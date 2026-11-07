# 权限与用户

noa 的鉴权是「单 token、双通道」：REST 与 git 共用同一套凭据，但写权限各自执法。

## 凭据

- **引导 token**：`NOA_API_TOKEN` 环境变量。服务端启动强制非空（空/全空白拒绝启动），
  该 token 即 admin 身份。
- **注册用户 token**：经 REST 创建（见下），sha256 哈希存储，常数时间比较。

认证形态（两通道一致）：

```text
Authorization: Bearer <token>
Authorization: Basic <base64(any-user:token)>     # git 客户端惯例：用户名任意
```

空 token 服务端（`NOA_API_TOKEN=""`）不会退化为匿名可读——启动即拒绝，且比较前的
非空守卫双重防御。

## 角色

| 角色 | 读 | 写（REST / git push） | 管理（用户 CRUD） |
|---|---|---|---|
| admin | ✓ | ✓ | ✓ |
| operator | ✓ | ✓ | ✗ |
| reader | ✓ | ✗ | ✗ |

- REST：中间件集中执法——reader 的写请求 403；`/api/v1/users` 非 GET 403（仅 admin）。
- git：push（receive-pack）要求 can_write——无凭据 401（带 Basic challenge，git 客户端
  据此提示）；有凭据但 reader 403（提示需要 write 角色）。clone/fetch 任意已认证身份。

## 用户管理 REST

```bash
# 创建（admin）
curl -X POST -H "Authorization: Bearer $ADMIN" -H "Content-Type: application/json" \
  -d '{"name":"alice","role":"operator","token":"<alice-token>"}' \
  http://127.0.0.1:3016/api/v1/users

# 列表（admin）／单个
curl -H "Authorization: Bearer $ADMIN" http://127.0.0.1:3016/api/v1/users
curl -H "Authorization: Bearer $ADMIN" http://127.0.0.1:3016/api/v1/users/alice

# 删除（admin）
curl -X DELETE -H "Authorization: Bearer $ADMIN" \
  http://127.0.0.1:3016/api/v1/users/alice
```

规则：

- `name` 与 `role`（admin/operator/reader）必填；token 服务端存储前做 sha256。
- 更新（同端点 POST 同名）可轮换 token 或改角色；`created_us` 保留原值。
- 删除是 admin 专属且不可逆。

## git 通道示例

```bash
# 读（任意已认证身份；公开仓用占位用户名即可）
git clone http://<token>@noa.example.com:3016/git/noa

# 写（需要 operator/admin）
git push origin master
```

未认证 push 收到 401 + Basic challenge（git 客户端据此提示凭据）；reader 身份 push 收到
403（提示需要 write 角色）。
