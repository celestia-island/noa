# 아키텍처 개요

noa는 에이전트 네이티브 분산 버전 관리 시스템입니다. "git의 래퍼"가 아닙니다:
코어 객체 모델은 기본적으로 SHA-256 콘텐츠 주소 방식이면서 git의 객체 대수와
바이트 단위 호환을 유지합니다(같은 파일 내용은 noa와 git 양쪽에서 동일한
blob OID로 해시됩니다 — 미러 충실도 검증이 정확히 이에 의존합니다).

## 3계층 모델

```
Workspace          — one isolated workspace per agent, invisible to each other
   │ head points to
Snapshot           — content-addressed commit (SHA-256), DAG structure (multiple parents = merge)
   │ tree_hash points to
ObjectStore        — blob / tree / symlink / gitlink (blobs byte-compatible with git;
                     trees/commits use noa's own SHA-256 addressing)
```

- **워크스페이스**는 noa의 핵심 격리 단위입니다: 같은 리포지토리 안에서 여러
  에이전트가 서로를 덮어쓰지 않고 각자의 워크스페이스와 head를 가집니다.
  `mirror.<job>.<branch>` 같은 시스템 네임스페이스도 워크스페이스 형태로
  존재합니다.
- **스냅샷**은 커밋에 해당합니다: `tree_hash` + `parents[]` + `author` +
  `timestamp` + `message`. 스냅샷 ID는 `noa_<sha256>` 형태입니다(콘텐츠
  주소 방식이므로 같은 트리 해시의 재임포트는 멱등입니다).
- **ref** (RedbRefStore): `HEAD`, `mirror/{job}/{branch}`,
  `mirror/{job}/tags/{tag}`, `mirror/{job}/pr/{N}` 등 — 전부 스냅샷 ID를
  가리키는 redb의 ref 행입니다.

스토리지 엔진은 단일 파일 임베디드 KV 스토어(redb)입니다 — 배포는 디렉터리
복사면 충분하고 외부 데이터베이스가 필요 없습니다.

## 서버 구성 요소 (noa-server)

| 구성 요소 | 담당 |
|---|---|
| REST API (`/api/v1/*`) | 리포지토리 / PR / runs / 사용자 / 미러 job의 CRUD |
| git-http (`/git/{repo}/*`) | 스마트 HTTP: upload-pack (clone/fetch) + receive-pack (push, W2a2부터) |
| 미러 엔진 | 주기 웨이브: 오리진에서 master / tags / PR heads를 fetch해 noa로 임포트; 백플로우 푸시 가능 |
| 실행 기록 | CI 실행 기록 저장소 (chest DevOps 패널의 데이터 소스) |
| 인증 미들웨어 | 라우트 단위 베어러 인증 + 역할 강제 (REST), can_write 강제 (git push) |

서버 상태는 전부 하나의 redb 데이터베이스에 있으며, 서버는 "디렉터리가 곧
전부"일 정도로 외부 상태 없이 동작합니다.

## git 호환성의 정확한 경계

호환성은 계층별로 정확히 말합니다:

- **blob: 바이트 단위 호환.** 원시 콘텐츠가 곧 git blob 본문이며
  `git_blob_oid(content)`가 git과 일치합니다(미러 충실도 검증이 정확히 이걸로
  blob 단위 대사를 합니다);
- **tree: 의미적으로 동등, ID는 동등하지 않음.** path/kind/blob별 OID는 git과
  정확히 대응하지만, noa의 tree ID는 자체 SHA-256 콘텐츠 주소 방식이며 git의
  트리 객체 형식이 아닙니다;
- **commit: noa 자체 모델.** 스냅샷 ID는 `noa_<sha256>` 형태이며 git 커밋
  객체가 아닙니다.

git 형태의 표현은 **미러 스크래치의 실제 git 리포지토리** 안에 존재합니다.
백플로우 푸시는 스크래치의 git 객체를 그대로 사용합니다(git에서 가져온
콘텐츠는 변환 제로). **noa 네이티브로 생성된 콘텐츠**(스크래치를 한 번도
거치지 않은 순수 워크스페이스 스냅샷)는 git 객체 형태가 없어 직접 백플로우에는
스냅샷→git 객체 직렬화기가 필요합니다 — 알려진 제한입니다(현재 구현에서는
이런 ref가 백플로우 푸시에 나타나지 않습니다).

## 미러 스크래치의 역할

`NOA_MIRROR_SCRATCH_DIR/{job}.git`은 미러 job마다 하나의 베어 클론이며 세
가지 역할을 동시에 합니다:

1. **fetch 소스 캐시** — 웨이브가 오리진에서 fetch하고, 임포터가 여기서 git
   객체를 읽습니다;
2. **git-http 서빙 본체** — `/git/{repo}`가 여기에 직접 upload-pack /
   receive-pack을 실행합니다;
3. **백플로우 푸시 소스** — 백플로우 레인이 로컬 ref를 오리진으로 원자적으로
   되돌려 푸시합니다.

푸시(receive-pack)는 스크래치의 로컬 ref를 갱신하고, 다음 웨이브가 이를 noa의
ref 체계로 임포트하며, 백플로우 레인이 그 상태를 오리진으로 되돌려 푸시합니다
— 세 경로가 하나의 스크래치를 공유하며 루프가 닫힙니다.

## 디렉터리 색인

- [빠른 시작](./getting-started.md)
- [미러와 백플로우](./mirror.md)
- [권한과 사용자](./rbac.md)
- [git-http 프로토콜](./git-http.md)
- [머지와 충돌](./merge.md)
- [CI 실행 기록](./ci-runs.md)
- [HTTP API 레퍼런스](../reference/http-api.md)
- [CLI 레퍼런스](../reference/cli.md)
- [배포](../reference/deploy.md)
