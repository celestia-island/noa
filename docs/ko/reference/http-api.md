# HTTP API 레퍼런스

Base URL: `http://<host>:<port>/api/v1`. 모든 엔드포인트는 베어러 인증
(`Authorization: Bearer <token>`)을 요구합니다; 인증 없는 요청은 401 +
`Basic` challenge를 받습니다. 역할 강제: reader의 쓰기 요청은 403;
`/users`의 non-GET은 관리자 전용입니다.

오류는 균일하게 `{"error": "..."}` 형태입니다; 페이지네이션은 인라인입니다
(`?limit=` 1..=1000, `?offset=`).

## repos — 리포지토리 카탈로그

| 메서드 | 경로 | 설명 |
|---|---|---|
| GET | `/repos` | 카탈로그 목록 |
| POST | `/repos` | 등록 (`name` 필수, 미러 job 이름과 같은 형태) |
| GET | `/repos/{name}` | 단건 |
| DELETE | `/repos/{name}` | 삭제 (미러 job이 살아 있거나 열린 PR이 있는 동안 409) |
| GET | `/repos/{name}/prs` | 리포지토리 범위 PR 목록 |
| POST/GET | `/repos/{name}/runs` | CI 실행 기록 ([CI 실행 기록](../guide/ci-runs.md) 참조) |

## prs — 풀 리퀘스트

| 메서드 | 경로 | 설명 |
|---|---|---|
| GET | `/prs?state=&base=&repo=&limit=&offset=` | 전역 목록 (`state` ∈ open/closed/merged) |
| POST | `/prs` | 생성 (title, repo, base, head 등) |
| GET | `/prs/{number}` | 단건 |
| POST | `/prs/{number}/merge` | 머지 (스쿼시/패스트 포워드; 충돌 시 409 + 충돌 목록) |

리포지토리 범위 변형: `GET /repos/{name}/prs` — 경로가 리포지토리를
결정하며, 모순되는 `?repo=` 필터는 400을 반환합니다.

## users — 사용자와 역할 (관리자)

| 메서드 | 경로 | 설명 |
|---|---|---|
| GET | `/users` | 목록 (관리자) |
| POST | `/users` | 생성/갱신 (같은 이름 = 토큰 순환 또는 역할 변경; `created_us` 보존) |
| GET | `/users/{name}` | 단건 (관리자) |
| DELETE | `/users/{name}` | 삭제 (관리자, 되돌릴 수 없음) |

역할: `admin`(전부) / `operator`(읽기+쓰기) / `reader`(읽기 전용).
필드와 검증: [권한과 사용자](../guide/rbac.md) 참조.

## mirror — 미러 job

| 메서드 | 경로 | 설명 |
|---|---|---|
| GET | `/mirror/jobs` | 전체 job 목록 (`last_status` / `last_lane` / `last_detail` 포함) |
| POST | `/mirror/jobs` | 생성/갱신 (같은 이름 검증 후 job 전체를 재설정) |
| DELETE | `/mirror/jobs/{name}` | job 삭제 |
| POST | `/mirror/run` | 수동 트리거 (본문 `{"job":"<name>"}`, 202 비동기) |

필드, 검증, 레인 시맨틱스: [미러와 백플로우](../guide/mirror.md) 참조.

## git-http — 스마트 프로토콜

`/git/{repo}/*`: clone/fetch (upload-pack)와 push (receive-pack). 인증,
동작 상세, 경계: [git-http 프로토콜](../guide/git-http.md) 참조.

## 공통 규약

- 모든 목록 엔드포인트에서 `limit` 기본 50, 상한 1000 (`limit=0` → 400);
- 페이지네이션 매개변수는 쿼리 문자열에 **인라인**입니다(중첩 래퍼 객체
  없음);
- 서버가 할당하는 필드(`number`, `started_us` 등)는 서버가 설정합니다;
  클라이언트가 넣은 값은 덮어써집니다;
- 오류는 내부 상세를 누설하지 않습니다(상수 `internal server error`
  형태); 상세는 서버 로그로 갑니다.
