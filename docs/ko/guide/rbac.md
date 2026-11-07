# 권한과 사용자

noa의 인증은 "하나의 토큰, 두 개의 채널"입니다: REST와 git이 같은 자격
증명을 공유하지만, 쓰기 권한은 각 채널에서 별도로 강제됩니다.

## 자격 증명

- **부트스트랩 토큰**: `NOA_API_TOKEN` 환경 변수. 서버는 값이 비어 있으면
  (빈 값 / 공백뿐) 시작 시점에 거부합니다; 이 토큰이 곧 관리자 신원입니다.
- **등록된 사용자 토큰**: REST로 생성(아래 참조), sha256 해시로 저장,
  상수 시간 비교.

인증 형태(양쪽 채널에서 동일):

```text
Authorization: Bearer <token>
Authorization: Basic <base64(any-user:token)>     # git client convention: any username
```

토큰이 빈 서버(`NOA_API_TOKEN=""`)는 익명 읽기로 절대 저하하지 않습니다 —
시작 자체가 거부되고, 비교 전 비어 있지 않음 검사가 방어 깊이를 더합니다.

## 역할

| 역할 | 읽기 | 쓰기 (REST / git push) | 관리 (사용자 CRUD) |
|---|---|---|---|
| admin | ✓ | ✓ | ✓ |
| operator | ✓ | ✓ | ✗ |
| reader | ✓ | ✗ | ✗ |

- REST: 강제는 미들웨어에 중앙화되어 있습니다 — reader의 쓰기 요청은 403;
  `/api/v1/users`의 non-GET은 403(관리자 전용)입니다.
- git: 푸시(receive-pack)는 can_write를 요구합니다 — 자격 증명이 없으면
  401(`Basic` challenge와 함께, git 클라이언트가 이를 프롬프트에 사용합니다);
  자격 증명이 있지만 reader면 403(쓰기 역할이 필요하다는 안내).
  clone/fetch는 인증된 모든 신원에 동작합니다.

## 사용자 관리 REST

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

규칙:

- `name`과 `role`(admin/operator/reader)은 필수입니다; 서버가 토큰을 저장
  전에 sha256 해시합니다.
- 갱신(같은 엔드포인트에 같은 이름으로 POST)은 토큰을 순환시키거나 역할을
  변경할 수 있습니다; `created_us`는 원래 값이 유지됩니다.
- 삭제는 관리자 전용이며 되돌릴 수 없습니다.

## git 채널 예

```bash
# Read (any authenticated identity; public repos work with a placeholder username)
git clone http://<token>@noa.example.com:3016/git/noa

# Write (requires operator/admin)
git push origin master
```

인증 없는 푸시는 401 + `Basic` challenge를 받고(git 클라이언트가 이를
프롬프트에 사용합니다), reader 신원의 푸시는 403을 받습니다(쓰기 역할이
필요하다는 안내).
