# 미러와 백플로우

미러 시스템은 외부 호스트(GitHub 등)에서 리포지토리의 master, 전체 tags,
활성 PR heads를 noa로 유입합니다; 백플로우 레인은 noa의 상태를 오리진으로
되돌려 푸시합니다 — "noa가 권위이고 GitHub는 미러"인 전환 형태가 만들어지고,
farm/CNB는 변경 없이 계속 GitHub에서 fetch합니다.

## 미러 job (MirrorJob)

```json
{
  "name": "noa",
  "remote_url": "https://github.com/celestia-island/noa.git",
  "github_slug": "celestia-island/noa",
  "branches": ["master"],
  "tags": true,
  "pr_heads": true,
  "backflow": true,
  "canary_push": true,
  "enabled": true
}
```

| 필드 | 설명 |
|---|---|
| `name` | `^[a-z0-9][a-z0-9-]{0,63}$`; 스크래치 디렉터리 이름, 카탈로그 리포지토리 이름, 클론 경로 세그먼트이기도 합니다 |
| `remote_url` | `https://` / `file://`만 허용; 자격 증명이 포함된 형태(`@`, `?`)는 거부 |
| `github_slug` | `owner/repo` 형태 검증; Git Data API 레인(tags / PR head 가져오기)과 canary 프로브에서 사용. 백플로우 푸시는 `remote_url` 사용 |
| `branches` | 최소 1개; `tags`/`tags/*`, `pr/*` (PR 네임스페이스 충돌), 그리고 파생 워크스페이스의 잘못된 형태는 모두 거부 |
| `tags` | 전체 tags를 미러 |
| `pr_heads` | 활성 PR heads를 미러 (`refs/pull/<N>/head` → `mirror/{job}/pr/<N>`) |
| `backflow` | 매 웨이브마다 스크래치 상태를 오리진으로 푸시 (아래 설명) |
| `canary_push` | 쓰기 방향을 검증하는 프로브 ref 하나를 푸시 |
| `enabled` | 꺼져 있으면 스케줄러가 job을 건너뜀 |

CRUD: `POST/GET /api/v1/mirror/jobs`, `DELETE /api/v1/mirror/jobs/{name}`,
`POST /api/v1/mirror/run` (수동 트리거, 본문 `{"job":"<name>"}`).

## 두 개의 레인

- **git 레인**: 베어 스크래치 클론 + `git fetch` — tags, PR heads,
  백플로우 푸시가 모두 이 레인을 통과합니다.
- **API 레인**: git 전송이 끊기면(네트워크 지터 계열) Git Data API를 통한
  blob 단위 임포트로 자동 저하하며, 모든 blob의 SHA를 검증합니다(충실도는
  레인이 바뀌어도 떨어지지 않습니다).

저하는 웨이브 단위입니다: `ensure_scratch` 실패 → 이번 웨이브는 API 레인으로
→ 셀프힐이 반쯤 깨진 스크래치를 삭제하고 다음 웨이브가 재클론합니다.
백플로우 푸시는 git 레인이 살아 있을 때만 실행됩니다(API로는 ref를 푸시할 수
없습니다).

## PR heads

`pr_heads: true`이면 매 웨이브가 추가로 `refs/pull/<N>/head`를 fetch하여
`mirror/{job}/pr/<N>` + 같은 이름의 워크스페이스로 임포트합니다. PR
네임스페이스와 브랜치 네임스페이스(`pr/<x>` 형태의 브랜치)는 상호 배타적이며
upsert에서 거부됩니다.

## 백플로우

```text
scratch local refs ──atomic push──▶ origin
  +refs/heads/*:refs/heads/*
  +refs/tags/*:refs/tags/*
  +refs/pull-heads/*:refs/noa/pr/*     (only when pull-heads exist in the scratch)
```

- **자격 증명**: `token_env`(기본 `NOA_MIRROR_GH_TOKEN`)에서 해석된 토큰을
  `x-access-token:<token>` 형태로 일회용 푸시 URL에 넣습니다; 토큰이
  스크래치 설정에 남지 않습니다.
- **force 시맨틱스**: 전환 후 noa가 쓰기 권위이며 오리진의 ref는 스크래치로
  수렴합니다. 삭제 비대칭(noa 측 삭제가 오리진 측 ref를 지우지 않음)은
  알려진 한계입니다.
- **실패 시맨틱스**: 실패한 백플로우는 웨이브를 FAILED로 표시합니다 — 수용
  기준의 핵심 레인이므로 반드시 눈에 띄어야 합니다.
- **네임스페이스 브리지**: GitHub는 `refs/pull/*` 푸시를 거부하므로 PR
  heads는 `refs/noa/pr/<N>`에 반영됩니다.

## 카나리 프로브

`canary_push: true`이면 매 웨이브가 `noa-mirror-canary` 프로브 ref(첫 복제
브랜치의 커밋)를 오리진으로 푸시합니다 — 먼저 git 전송을 시도하고 실패 시
Git Data API로 저하 — 쓰기 방향의 자격 증명과 연결 상태를 계속 검증합니다.
프로브가 실패하면 웨이브가 크게 실패 처리됩니다.

## 충실도 검증

임포트된 모든 커밋을 항목 단위로 교차 검증합니다: path 집합, 엔트리 종류,
blob OID(`git_blob_oid`가 git과 일치)가 정확히 일치해야 하며, 하나라도
다르면 해당 항목이 실패 처리되어 웨이브의 오류 상세에 기록됩니다. 백플로우는 이 불변 조건에 의존합니다: git에서 가져온 콘텐츠의 경우 스크래치의 git 객체와 noa 객체는 blob 계층에서 1:1로 대응하며 푸시에 변환이 필요 없습니다. **경계**: noa 네이티브로 생성된 커밋(순수 워크스페이스 스냅샷)에는 git 객체 형태가 없어 백플로우 대상이 아닙니다(정확한 경계는 [아키텍처](./architecture.md) 참조).

## stale ref 정리 (reconcile)

tags와 PR head의 업스트림 집합이 정리(reconcile) 대상입니다: 더 이상
업스트림에 나타나지 않는 ref는 noa에서 제거됩니다(`mirror/{job}/tags/`와
`mirror/{job}/pr/` 네임스페이스). tags 백플로우 푸시도 마찬가지로 업스트림을
따릅니다(noa 측 삭제가 오리진 측을 지우지 않는 것은 알려진 비대칭입니다).

## 스케줄링

`NOA_MIRROR_ENABLED=1`이면 스케줄러가 활성 job 전체를 900초마다 한 번씩
실행합니다; `POST /mirror/run`이 수동 트리거입니다. 웨이브 상세(레인,
항목별 결과, 백플로우/카나리 상태)는 `GET /api/v1/mirror/jobs`의
`last_detail` / `last_lane`에서 확인할 수 있습니다.
