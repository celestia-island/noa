# git-http 프로토콜

`/git/{repo}/*`는 스마트 HTTP 프로토콜을 서빙합니다: 읽기 경로는
`git upload-pack --stateless-rpc`를 직접 구동하고, 쓰기 경로(W2a2부터)는
`git receive-pack --stateless-rpc`를 직접 구동합니다 — `git-http-backend`
CGI 래퍼 없이 말입니다. 따라서 프로토콜 협상, 환경 변수, 오류 형태를 완전히
통제합니다.

## 엔드포인트

| 메서드 | 경로 | 용도 |
|---|---|---|
| GET | `/git/{repo}/info/refs?service=git-upload-pack` | clone/fetch 광고 |
| POST | `/git/{repo}/git-upload-pack` | pack 협상 |
| GET | `/git/{repo}/info/refs?service=git-receive-pack` | 푸시 광고 |
| POST | `/git/{repo}/git-receive-pack` | 푸시 수용 |

`{repo}`는 미러 job 이름이어야 합니다 — 서빙 본체는 그 job의 베어 스크래치
클론(`NOA_MIRROR_SCRATCH_DIR/{repo}.git`)입니다. 존재하지 않는 리포지토리는
404를 반환합니다.

## 동작 상세

- **프로토콜 협상**: 클라이언트의 `Git-Protocol` 헤더가 자식 프로세스로
  전달됩니다(요청 시 v2 활성화; 빈 값은 기본 v0). 직접 구동 구현이
  git-http-backend를 대체한 이유가 정확히 이것입니다 — CGI 래퍼는 POST
  경로에서 이 헤더를 떨어뜨려 조용히 v0로 강등하고 클론을 깨뜨립니다.
- **광고 프리앰블**: `# service=` pkt-line(서비스 이름에서 길이 헤더를 동적으로
  계산) + flush; RPC 응답에는 프리앰블이 없습니다.
- **Content-Type**: `application/x-git-{upload,receive}-pack[-advertisement]`을
  엄격히 구분합니다.
- **요청 본문**: POST 본문이 자식 프로세스의 stdin으로 곧장 기록됩니다;
  `Content-Encoding: gzip`은 자동으로 해제됩니다(해제 비율 1000:1 초과는
  거부 — bomb 방어; 요청 50 MiB / 해제 후 200 MiB 상한).
- **타임아웃**: 전체 교환에 60초 wall clock이 적용됩니다; 타임아웃 시 자식
  프로세스를 kill합니다(kill_on_drop가 고아 lock 보유자를 막습니다).
- **동시성**: 라우트 단위 세마포어가 진행 중인 git 교환을 제한합니다(메모리
  천장 4×50 MiB).
- **자식 프로세스 환경**: `env_clear` 뒤에는 `PATH` + `GIT_PROTOCOL`만
  남습니다 — 환경 누출이 구조적으로 불가능합니다.

## 푸시 시맨틱스

푸시는 스크래치의 로컬 ref에 반영되고, 이어서 미러 웨이브가:

1. 전진한 스크래치 ref를 noa의 ref 체계(스냅샷/워크스페이스)로 임포트하고;
2. `backflow`가 활성화된 경우 그 상태를 오리진으로 다시 푸시합니다.

따라서 푸시에 특별한 훅이 필요 없습니다 — 세 레인(읽기, receive, 백플로우)이
하나의 스크래치를 공유하고 닫힌 루프가 자동으로 수렴합니다. 푸시에는 인증된
신원이 필요합니다; 자격 증명이 없으면 401 + `Basic` challenge, reader
역할이면 403입니다.

## 알려진 경계

- 푸시는 삭제 시맨틱스를 제공하지 않습니다(noa 측 삭제가 오리진으로 전파되지
  않음 — 알려진 비대칭).
- dumb HTTP(베어 `/info/refs` 파일)는 서빙하지 않습니다.
- 백플로우 시 PR heads는 `refs/noa/pr/<N>`에 반영됩니다(GitHub는
  `refs/pull/*` 푸시를 거부합니다).
- 백플로우 푸시의 소스는 스크래치의 git 객체입니다 — **noa 네이티브로 생성된
  커밋**(스크래치를 거치지 않은 순수 워크스페이스 스냅샷)은 스냅샷→git 객체
  직렬화기가 도착할 때까지 백플로우 범위 밖입니다([아키텍처](./architecture.md)
  참조).
