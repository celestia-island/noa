# 빠른 시작

이 문서는 0에서 실행 중인 noa까지를 안내합니다: 빌드하고, 서버를 시작하고,
첫 clone / push까지 완료합니다.

## 사전 요구 사항

- Rust stable ≥ 1.99 (`rustup`으로 설치)
- git ≥ 2.39 (서버의 스마트 HTTP가 `upload-pack` / `receive-pack`을 직접 구동)
- 선택 사항: node + pnpm (WebUI 빌드용)

## 빌드

```bash
git clone https://github.com/celestia-island/noa.git
cd noa
cargo build --release --locked
```

산출물: `target/release/noa` (CLI)와 `target/release/noa-server` (서버).

## 서버 시작

최소 설정에는 API 토큰 하나(부트스트랩 관리자 자격 증명)만 필요합니다:

```bash
export NOA_API_TOKEN="<your-token>"
export NOA_MIRROR_SCRATCH_DIR=/var/lib/noa/mirror-cache   # mirror scratch root
export NOA_MIRROR_ENABLED=1                               # enable mirror scheduling (default 900s)
export NOA_MIRROR_GH_TOKEN="<github-pat>"                 # optional: back-flow / canary push credential
noa-server --host 0.0.0.0 --port 3016
```

시작 후:

```bash
curl -H "Authorization: Bearer <your-token>" http://127.0.0.1:3016/api/v1/repos
```

`[]`가 반환되면 서비스가 준비된 것입니다. 모든 REST 엔드포인트가 베어러 인증을
요구하며, `/git/{repo}/*`의 읽기 경로도 마찬가지입니다(인증 없는 요청에는
`Basic` challenge와 함께 401이 반환됩니다).

## noa 네이티브 워크플로 (권장)

`noa` CLI가 최우선 입구입니다 — 클론해 오는 것은 `.git`과 `.noa`가 공존하는
리포지토리입니다(git은 소스를, noa는 에이전트 데이터를 관리합니다):

```bash
noa clone http://<token>@127.0.0.1:3016/git/hello --path hello
cd hello
noa workspace create agent-a     # a per-agent isolated workspace
# … edit files …
noa snapshot create -m "change" -a agent-a
noa log                          # this workspace's snapshot history
noa push                         # publishes the workspace snapshot (serialized to a git commit)
```

`noa push`가 게시하는 것은 **워크스페이스 스냅샷 상태**입니다(`[noa export]` 커밋을
생성) — `.git`의 커밋이 아닙니다. 두 레인은 병행하며 섞어 쓰지 마세요(푸시 전에
`noa snapshot create`를 먼저 실행하지 않으면 오래된 스냅샷이 게시됩니다). 인증은
`NOA_GIT_USER` + `NOA_GIT_PASS` 환경 변수에서 주입됩니다. 디렉터리 전체
(`.git/noa` 포함)를 다른 머신으로 복사해도 그대로 동작합니다.

## git 호환 레인 (모든 git 도구 사용 가능)

같은 `/git/{repo}` 엔드포인트가 표준 git 스마트 HTTP도 함께 처리합니다 — 순정 git,
CI, farm도 수정 없이 동작합니다:

```bash
git clone http://<token>@127.0.0.1:3016/git/hello
```

푸시(receive-pack)에는 쓰기 역할이 필요하며, 착지한 푸시는 **즉시 임포트 웨이브를
트리거합니다**(백플로우는 같은 웨이브에서 이어집니다). 이것이 farm/CNB를 무변경으로
유지하는 전환 형태입니다.

## 첫 미러 (GitHub 리포지토리를 noa로 유입하기)

```bash
curl -X POST -H "Authorization: Bearer <your-token>" \
  -H "Content-Type: application/json" \
  -d '{
    "name": "hello",
    "remote_url": "https://github.com/you/hello.git",
    "github_slug": "you/hello",
    "branches": ["master"],
    "tags": true,
    "pr_heads": true,
    "enabled": true
  }' \
  http://127.0.0.1:3016/api/v1/mirror/jobs

curl -X POST -H "Authorization: Bearer <your-token>" \
  -H "Content-Type: application/json" \
  -d '{"job":"hello"}' \
  http://127.0.0.1:3016/api/v1/mirror/run
```

웨이브가 끝나면 해당 리포지토리를 noa 경유로 클론할 수 있습니다:

```bash
git clone http://<your-token>@127.0.0.1:3016/git/hello
```

## 백업과 이전

서버 상태 전체를 하나의 이동 가능한 디렉터리로 내보낼 수 있습니다(자세한 내용은
[배포](../reference/deploy.md)의 백업 절): redb 일관성 스냅샷 + 리포지토리별 git
번들 + sha256 매니페스트. `noa-server --import-state <dir>`로 오프라인 복원합니다.
수용 기준은 리허설입니다: 내보내기 → 다른 머신에서 복원 → 구동 후 대조.


## 다음 단계

- [아키텍처 개요](./architecture.md) — 워크스페이스 / 스냅샷 / ref 모델
- [미러와 백플로우](./mirror.md) — 두 개의 레인, PR heads, 백플로우와 카나리
- [HTTP API 레퍼런스](../reference/http-api.md) — 전체 엔드포인트
