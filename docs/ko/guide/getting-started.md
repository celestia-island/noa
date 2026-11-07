# 빠른 시작

이 문서는 0에서 실행 중인 noa까지를 안내합니다: 빌드하고, 서버를 시작하고,
첫 clone / push까지 완료합니다.

## 사전 요구 사항

- Rust stable ≥ 1.99 (`rustup` 사용)
- git ≥ 2.39 (서버의 스마트 HTTP가 `upload-pack` / `receive-pack`을 직접
  구동합니다)
- 선택 사항: node + pnpm (WebUI 빌드용)

## 빌드

```bash
git clone https://github.com/celestia-island/noa.git
cd noa
cargo build --release --locked
```

산출물: `target/release/noa` (CLI)와 `target/release/noa-server` (서버).

## 서버 시작

최소 설정은 API 토큰 하나(부트스트랩 관리자 자격 증명)뿐입니다:

```bash
export NOA_API_TOKEN="<your-token>"
export NOA_MIRROR_SCRATCH_DIR=/var/lib/noa/mirror-cache   # mirror scratch root
export NOA_MIRROR_ENABLED=1                               # enable the scheduler (900s default)
export NOA_MIRROR_GH_TOKEN="<github-pat>"                 # optional: back-flow / canary credential
noa-server --host 0.0.0.0 --port 3016
```

시작이 끝나면:

```bash
curl -H "Authorization: Bearer <your-token>" http://127.0.0.1:3016/api/v1/repos
```

`[]`가 반환되면 준비 완료입니다. 모든 REST 엔드포인트가 베어러 인증을
요구하며, `/git/{repo}/*` 읽기도 마찬가지입니다(인증 없는 요청에는 `Basic`
challenge와 함께 401이 반환됩니다).

## 첫 미러 (GitHub 리포지토리를 noa로 가져오기)

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

웨이브가 완료되면 해당 리포지토리를 noa를 통해 클론할 수 있습니다:

```bash
git clone http://<your-token>@127.0.0.1:3016/git/hello
```

## 첫 푸시 (쓰기 권한)

푸시는 `/git/{repo}` receive-pack 경로로 처리되며 인증된 신원이 필요합니다:

```bash
cd hello
echo "hello noa" > README.md
git add -A && git commit -m "hello noa"
git push origin master
```

푸시는 미러 스크래치에 먼저 반영되고, 다음 미러 웨이브가 이를 noa의
스냅샷/ref 체계로 임포트합니다. job에 `backflow`가 활성화되어 있으면 같은
상태가 오리진(GitHub)으로도 다시 푸시됩니다 — "noa가 권위이고 GitHub는
미러"인 전환 형태입니다.

## 다음 단계

- [아키텍처 개요](./architecture.md) — 워크스페이스 / 스냅샷 / ref 모델
- [미러와 역방향 푸시](./mirror.md) — 듀얼 레인, PR heads, 역방향 푸시, canary
- [HTTP API 참조](../reference/http-api.md) — 전체 엔드포인트
