# CLI 레퍼런스

`noa`는 워크스페이스 쪽 명령줄 진입점입니다(clap 파생; 실제 시그니처는
`noa --help`). 모든 명령은 `noa init`으로 초기화된 리포지토리 안에서
실행됩니다.

## 명령어 개요

| 명령어 | 용도 |
|---|---|
| `noa init` | 현재 디렉터리를 noa 리포지토리로 초기화 (`.noa` 메타데이터 디렉터리 생성) |
| `noa status` | 현재 워크스페이스 상태: head, 커밋되지 않은 변경, 워크스페이스 목록 |
| `noa log` | 현재 워크스페이스의 스냅샷 히스토리 (축약 렌더링 + 부모 체인 순회) |
| `noa snapshot` | 스냅샷 작업 (생성 / 검사) |
| `noa workspace` | 워크스페이스 관리: `workspace create <name>` (에이전트별 격리), 전환, 목록 |
| `noa remote` | 원격 관리 (add / set-url / list) |
| `noa push` / `noa pull` / `noa fetch` / `noa clone` | 원격과 동기화 (git 호환 시맨틱스) |
| `noa resolve <path>` | 중첩 경로를 트리 엔트리로 해석 |
| `noa coauthor` | 커밋에 공동 저자를 붙임 (에이전트 협업 귀속) |
| `noa hook` | 리포지토리 훅 관리 (센티널 표시와 함께 기존 훅을 안전하게 덮어씀) |
| `noa verify` | 현재 워크스페이스의 에이전트 로그에 `verify.run/1` 기록을 추가 (stdin 또는 `--file`로 JSON) — 검증 장부의 CLI 기록기 |
| `noa sync` | 동기화 작업 |
| `noa storage` | 스토리지 백엔드 작업 |
| `noa pr` | PR 라이프사이클 (포지 비의존): `pr create` / `pr list` / `pr diff` 등 |

## 워크스페이스 격리 워크플로

```bash
noa init
noa workspace create agent-a      # agent A's isolated workspace
# … edit inside the workspace …
noa snapshot -m "agent-a's changes"
noa log                           # sees only this workspace's parent chain
```

여러 에이전트가 각자 `workspace create`를 실행하며 서로의 head와 콘텐츠에는
보이지 않습니다 — 이것이 noa와 전통적 브랜치 워크플로의 근본적 차이입니다:
격리 단위는 ref가 아니라 워크스페이스입니다.

## 서버와의 협업

CLI의 push/pull/fetch/clone은 서버의 `/git/*` 스마트 프로토콜과 호환됩니다
(같은 git 호환 시맨틱스). 원격이 noa-server를 가리키면:

```bash
noa remote add origin http://noa.example.com:3016/git/noa
noa push origin master
```

푸시는 receive-pack을 통과하며 인증된 신원이 필요합니다(토큰이 비밀번호를
겸하며, 역할에 쓰기 권한이 있어야 합니다).

## verify와 장부

`noa verify`는 검증 장부(verification ledger)의 CLI 기록기입니다: 기록은
`verify.run/1` 스키마로 현재 워크스페이스의 에이전트 로그(JSONL, 추가
전용)에 추가됩니다. 기록은 `hardware=<hw-id>` 바인딩을 가질 수 있습니다
(워크스페이스 쪽 하드웨어 레지스트리 감사와 함께 작동).

> 권위 있는 장부는 파일 쪽(`/mnt/work/verify-ledger/`)에 있습니다; CLI
> 기록은 fail-open 이중 기록입니다 — 기록하는 쪽이 체인을 검증하지 않는
> 것은 설계 판단이며, 체인 검증은 읽는 쪽이 수행합니다.
