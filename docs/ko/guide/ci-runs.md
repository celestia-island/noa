# CI 실행 기록 (runs API)

noa에는 CI 실행 기록 저장소가 내장되어 있습니다: "어떤 리포지토리의 어떤
head에 대해 실행된 검사 파이프라인" 하나가 run 하나입니다. 이것이 chest
DevOps 패널의 CI 뷰에 있는 `CI_BACKEND=noa` 레인의 데이터 소스입니다.

## 엔드포인트

| 메서드 | 경로 | 설명 |
|---|---|---|
| POST | `/api/v1/repos/{name}/runs` | run 하나를 기록 (쓰기 역할) |
| GET | `/api/v1/repos/{name}/runs?limit=&offset=` | 목록, 최신 우선 |

- `{name}`은 카탈로그에 존재해야 합니다(그렇지 않으면 404);
- `number`는 서버가 리포지토리별로 할당합니다(원자적: 스캔과 기록이 같은
  쓰기 트랜잭션에서 일어나므로 동시 기록이 서로의 번호를 덮어쓰지 않습니다);
- 검증: `head_sha`는 16진수 40자; `status` ∈ `queued | running | success |
  failure | cancelled`; `kind`는 1..=64자; `actor`는 1..=200; `log_ref` /
  `summary` ≤ 2000.

## run 기록하기

```bash
curl -X POST -H "Authorization: Bearer $TOKEN" -H "Content-Type: application/json" \
  -d '{
    "head_sha": "<40-hex>",
    "kind": "basic-check",
    "status": "success",
    "actor": "farm-runner-1",
    "log_ref": "https://ci.example.com/logs/123"
  }' \
  http://127.0.0.1:3016/api/v1/repos/noa/runs
```

응답(201): 서버가 `repo`, `number`, `started_us`(클라이언트가 제공하지 않은
경우)를 채웁니다.

## 조회

```bash
curl -H "Authorization: Bearer $TOKEN" \
  "http://127.0.0.1:3016/api/v1/repos/noa/runs?limit=10&offset=0"
```

반환된 행은 다음과 같은 모양입니다:

```json
{
  "repo": "noa",
  "number": 1,
  "head_sha": "…",
  "kind": "basic-check",
  "status": "success",
  "actor": "farm-runner-1",
  "started_us": 1794049465562618
}
```

## 설계 경계

- **로그는 noa에 저장되지 않습니다**: 기록은 디스패치 파이프라인의 로그
  저장소를 가리키는 불투명한 `log_ref` 포인터만 가집니다;
- **행 내용에는 제한이 있습니다**(검증 참조) — 인증된 작성자가 행 크기를
  부풀리는 것을 막습니다;
- **저장소**: redb `runs` 테이블, 키 `{repo}#{number:020}` (사전 순서 =
  리포지토리 내 시간 순서).

## chest 연동

chest의 `ci_ops` 핸들러는 `CI_BACKEND` 환경 변수를 읽습니다:

- `github`(기본): v1 GitHub Actions 레인 — 과거 버전과 동작이 동일합니다;
- `noa`: `NOA_API_BASE` + `NOA_API_TOKEN`이 가리키는 noa runs API를 읽고,
  행을 GitHub 행과 같은 패널 형태로 정규화합니다(`kind`→name,
  `number`→run_number, `started_us`→RFC3339 `created_at`, 종결 상태는
  conclusion에 매핑); 감시 목록은 `CI_NOA_REPOS`(단일 세그먼트 디렉터리
  이름)를 사용하며, unset이면 `CI_GITHUB_REPOS`의 마지막 세그먼트로
  폴백합니다.

패널의 `ci.config.get`은 `backend` 필드를 보고합니다; `configured`는
백엔드를 인지합니다 — 순수 noa 배포(GitHub 토큰 없음)도 noa 3종이 갖춰져
있으면 연결된 것으로 셈합니다.
