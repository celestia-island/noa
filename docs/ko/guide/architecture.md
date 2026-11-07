# 아키텍처 개요

noa는 AI 네이티브 분산 버전 관리 시스템입니다. "git 래퍼"가 아닙니다: 코어
객체 모델은 자체 SHA-256 콘텐츠 주소를 사용하면서 git의 객체 대수와
바이트 호환을 유지합니다(같은 파일 내용은 noa와 git에서 동일한 blob OID를
만들어냅니다 — 미러 정합성 검증이 정확히 이에 의존합니다).

## 3계층 모델

```
Workspace   —— one isolated workspace per agent; agents cannot see each other
   │ head points to
Snapshot    —— content-addressed commit (SHA-256), DAG structure (multi-parent = merge)
   │ tree_hash points to
ObjectStore —— blob / tree / symlink / gitlink, git-object-algebra compatible
```

- **워크스페이스**는 noa의 핵심 격리 단위입니다: 같은 리포지토리의 여러
  에이전트가 서로를 덮어쓰지 않고 각자의 워크스페이스와 head를 가집니다.
  `mirror.<job>.<branch>` 같은 시스템 네임스페이스도 워크스페이스로
  존재합니다.
- **스냅샷**은 커밋에 해당합니다: `tree_hash` + `parents[]` + `author` +
  `timestamp` + `message`. 스냅샷 ID는 `noa_<sha256>` 형태입니다(콘텐츠
  주소 방식이므로 변경 없는 트리의 재임포트는 멱등입니다).
- **ref** (RedbRefStore): `HEAD`, `mirror/{job}/{branch}`,
  `mirror/{job}/tags/{tag}`, `mirror/{job}/pr/{N}` — redb의 모든 ref 행이
  스냅샷 ID를 가리킵니다.

스토리지 엔진은 임베디드 단일 파일 KV 스토어(redb)입니다 — 배포는
"디렉터리 복사"면 충분하고 외부 데이터베이스가 필요 없습니다.

## 서버 구성 요소 (noa-server)

| 구성 요소 | 담당 |
|---|---|
| REST API (`/api/v1/*`) | 리포지토리 / PR / runs / 사용자 / 미러 job의 CRUD |
| git-http (`/git/{repo}/*`) | 스마트 HTTP: upload-pack (clone/fetch) + receive-pack (push, W2a2부터) |
| 미러 엔진 | 주기 웨이브: 오리진에서 master / tags / PR heads를 noa로 pull; 선택적으로 역방향 푸시 |
| 실행 기록 | CI 실행 기록 저장소 (chest DevOps 패널의 데이터 소스) |
| 인증 미들웨어 | 라우트 단위 베어러 인증 + 역할 강제 (REST), can_write 강제 (git push) |

서버 상태는 전부 하나의 redb 데이터베이스에 있습니다 — "디렉터리가 곧
전부"입니다.

## git 객체 호환성

임포트와 역방향 푸시는 모두 "git 객체 대수가 그대로"라는 가정 위에서
동작합니다:

- blob: 원시 콘텐츠가 곧 git blob 본문입니다 (`git_blob_oid(content)`가 git과
  일치);
- tree: 임포트 시 path/kind/oid를 엔트리 단위로 대조합니다 (정합성 검증);
- commit: 미러 웨이브가 부모 연결을 유지합니다 (`noa_<sha>` 콘텐츠 주소 +
  부모 엣지).

따라서 noa → git 역방향 푸시에는 변환 계층이 전혀 필요 없습니다: 스크래치의
git 객체와 noa의 객체가 1:1로 대응합니다.

## 미러 스크래치의 역할

`NOA_MIRROR_SCRATCH_DIR/{job}.git`은 미러 job마다 하나의 베어 클론이며 세
가지 역할을 합니다:

1. **pull 소스 캐시** — 웨이브가 오리진에서 fetch하고, 임포터가 여기서 git
   객체를 읽습니다;
2. **git-http 서빙 본체** — `/git/{repo}`가 여기에 직접 upload-pack /
   receive-pack을 실행합니다;
3. **역방향 푸시 소스** — 역방향 레인이 로컬 ref를 오리진으로 원자적으로
   푸시합니다.

푸시(receive-pack)는 스크래치의 로컬 ref를 갱신하고, 다음 웨이브가 이를 noa의
ref 체계로 임포트하며, 역방향 레인이 그 상태를 오리진으로 다시 푸시합니다 —
세 경로가 하나의 스크래치를 공유하며 루프는 스스로 닫힙니다.

## 디렉터리 색인

- [빠른 시작](./getting-started.md)
- [미러와 역방향 푸시](./mirror.md)
- [RBAC](./rbac.md)
- [git-http 프로토콜](./git-http.md)
- [머지와 충돌](./merge.md)
- [CI 실행 기록](./ci-runs.md)
- [HTTP API 참조](../reference/http-api.md)
- [CLI 참조](../reference/cli.md)
- [배포](../reference/deploy.md)
