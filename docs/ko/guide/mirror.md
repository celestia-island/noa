# 미러와 백플로우

미러 시스템은 외부 리포지토리(GitHub 등)의 master, 전체 tags, 활성 PR heads를 noa로 가져옵니다. 백플로우 레인은 noa의 상태를 origin으로 다시 푸시합니다 — 'noa 권위 + GitHub 미러'의 과도기 형태를 이루며, farm/CNB는 그대로 GitHub에서 fetch하므로 변경은 제로입니다.

## 미러 잡(MirrorJob)

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
| `name` | `^[a-z0-9][a-z0-9-]{0,63}$`. scratch 디렉터리 이름, catalog 리포지토리 이름, 클론 경로 세그먼트를 겸함 |
| `remote_url` | `https://` / `file://`만 허용. 자격 증명 내장(`@`, `?`) 금지 |
| `github_slug` | `owner/repo` 형태 검증. Git Data API 우회와 백플로우에서 사용 |
| `branches` | 최소 1개. `tags`/`tags/*`, `pr/*`(PR 네임스페이스 충돌)와 파생 워크스페이스의 부적합 형태는 모두 거부 |
| `tags` | 전체 tags 미러링 |
| `pr_heads` | 활성 PR heads 미러링(`refs/pull/<N>/head` → `mirror/{job}/pr/<N>`) |
| `backflow` | 웨이브마다 scratch 상태를 origin으로 푸시(아래 참조) |
| `canary_push` | 쓰기 방향을 검증하는 카나리아 참조 1개를 푸시 |
| `enabled` | 끄면 스케줄러가 건너뜀 |

CRUD: `POST/GET /api/v1/mirror/jobs`, `DELETE /api/v1/mirror/jobs/{name}`, `POST /api/v1/mirror/run`(수동 트리거, body는 `{"job":"<name>"}`).

## 두 개의 레인

- **git 레인**: scratch 베어 클론 + `git fetch` — tags, PR heads, 백플로우 푸시가 모두 이 레인을 사용합니다.
- **API 레인**: git 전송이 죽었을 때(네트워크 흔들림 계열) 자동으로 Git Data API의 blob 단위 가져오기로 강등합니다. 각 blob의 SHA를 검증합니다(충실도는 레인에 따라 낮아지지 않습니다).

강등은 웨이브 단위입니다: `ensure_scratch` 실패 → 해당 웨이브는 API 레인 사용 → 자가 치유로 반쯤 깨진 scratch를 삭제하고 다음 웨이브에서 다시 클론. 백플로우 푸시는 git 레인이 사용 가능할 때만 실행됩니다(API로는 참조를 푸시할 수 없습니다).

## PR heads

`pr_heads: true`이면 웨이브마다 추가로 `refs/pull/<N>/head`를 가져와 `mirror/{job}/pr/<N>` + 같은 이름의 워크스페이스로 가져옵니다. PR 네임스페이스와 브랜치 네임스페이스(`pr/<x>` 형태의 브랜치)는 상호 배타적이며, upsert 시 거부합니다.

## 백플로우(backflow)

```text
scratch 本地引用 ──atomic push──▶ origin
  +refs/heads/*:refs/heads/*
  +refs/tags/*:refs/tags/*
  +refs/pull-heads/*:refs/noa/pr/*     （仅当 scratch 存在 pull-heads）
```

- **자격 증명**: `token_env`(기본값 `NOA_MIRROR_GH_TOKEN`)에서 해석한 토큰을 `x-access-token:<token>` 형태로 일회성 push URL에 넣습니다. 토큰은 scratch 설정에 남지 않습니다.
- **force 시맨틱스**: 전환 후 noa가 쓰기 권위이며, origin의 참조가 scratch로 수렴합니다. 삭제의 비대칭(noa 측 삭제는 origin 측 참조를 삭제하지 않음)은 알려진 제한입니다.
- **실패 시맨틱스**: 백플로우 실패는 해당 웨이브를 FAILED로 만듭니다 — 이것은 검수 핵심 레인이므로 눈에 띄어야 합니다.
- **네임스페이스 브리지**: GitHub은 `refs/pull/*` 푸시를 거부하므로 PR heads는 `refs/noa/pr/<N>`에 놓입니다.

## 카나리아 프로브

`canary_push: true`이면 웨이브마다 origin에 `noa-mirror-canary` 카나리아 참조 1개(복제된 첫 브랜치의 커밋)를 푸시합니다. 먼저 git 전송을 시도하고 실패 시 Git Data API로 강등합니다 — 쓰기 방향의 자격 증명과 연결성을 지속 검증하기 위한 것입니다. 프로브 실패는 웨이브를 뚜렷한 실패로 만듭니다.

## 충실도 검증

가져온 커밋마다 항목별 대조를 수행합니다: 경로 집합, 항목 kind, blob OID(`git_blob_oid`는 git과 일치)가 완전히 일치해야 합니다. 어떤 차이든 해당 항목을 실패 처리하고 웨이브 오류 상세에 기록합니다. 백플로우는 이 불변량에 의존합니다 — noa 객체와 git 객체가 1:1로 대응하므로 푸시에 변환 계층이 필요 없습니다.

## 만료 참조 정리(reconcile)

tags와 PR heads의 업스트림 집합이 reconcile 집합입니다. 더 이상 업스트림에 나타나지 않는 참조는 noa에서 정리됩니다(`mirror/{job}/tags/`, `mirror/{job}/pr/` 네임스페이스). 백플로우의 tags 푸시도 업스트림을 기준으로 합니다(noa 측 삭제는 origin 측을 삭제하지 않음 — 알려진 비대칭).

## 스케줄링

`NOA_MIRROR_ENABLED=1`이면 스케줄러가 900초마다 활성 잡 전체를 한 바퀴 실행합니다. `POST /mirror/run`으로 수동 트리거할 수도 있습니다. 웨이브 상세(레인, 항목별 결과, backflow/canary 상태)는 `GET /api/v1/mirror/jobs`의 `last_detail` / `last_lane`을 참조하세요.
