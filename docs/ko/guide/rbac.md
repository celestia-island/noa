# 권한과 사용자

noa의 인증은 '단일 토큰, 이중 채널' 방식입니다: REST와 git이 같은 자격 증명을 공유하지만, 쓰기 권한은 각자 집행합니다.

## 자격 증명

- **부트스트랩 토큰**: `NOA_API_TOKEN` 환경 변수. 서버는 시작 시 비어 있지 않음을 강제합니다(빈 값/공백뿐이면 시작 거부). 이 토큰이 곧 admin 신원입니다.
- **등록 사용자 토큰**: REST로 생성(아래 참조). sha256 해시로 저장하고 상수 시간 비교를 사용합니다.

인증 형태(두 채널 동일):

```text
Authorization: Bearer <token>
Authorization: Basic <base64(any-user:token)>     # git 客户端惯例：用户名任意
```

빈 토큰 서버(`NOA_API_TOKEN=""`)는 익명 읽기 허용으로 약화되지 않습니다 — 시작 자체를 거부하며, 비교 전 빈 값 방어 가드가 이중 방어를 이룹니다.

## 역할

| 역할 | 읽기 | 쓰기(REST / git push) | 관리(사용자 CRUD) |
|---|---|---|---|
| admin | ✓ | ✓ | ✓ |
| operator | ✓ | ✓ | ✗ |
| reader | ✓ | ✗ | ✗ |

- REST: 미들웨어가 중앙에서 집행합니다 — reader의 쓰기 요청은 403. `/api/v1/users`의 비 GET은 403(admin 전용).
- git: push(receive-pack)는 can_write를 요구합니다 — 자격 증명 없으면 401(Basic challenge 포함, git 클라이언트가 이를 근거로 안내). 자격 증명은 있으나 reader면 403(write 역할 필요 안내). clone/fetch는 인증된 신원이면 누구나 가능합니다.

## 사용자 관리 REST

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

규칙:

- `name`과 `role`(admin/operator/reader)은 필수입니다. 토큰은 서버가 저장 전 sha256 처리를 합니다.
- 업데이트(같은 엔드포인트에 같은 이름으로 POST)로 토큰 로테이션 또는 역할 변경이 가능합니다. `created_us`는 원래 값을 유지합니다.
- 삭제는 admin 전용이며 되돌릴 수 없습니다.

## git 채널 예시

```bash
# 读（任意已认证身份；公开仓用占位用户名即可）
git clone http://<token>@noa.example.com:3016/git/noa

# 写（需要 operator/admin）
git push origin master
```

인증되지 않은 push는 401 + Basic challenge를 받습니다(git 클라이언트가 이를 근거로 자격 증명을 안내합니다). reader 신원의 push는 403을 받습니다(write 역할 필요 안내).
