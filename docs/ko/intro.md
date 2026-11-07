<p align="center"><img src="https://raw.githubusercontent.com/celestia-island/docs.celestia.world/dev/res/logo/noa.webp" alt="noa" width="200" /></p>

<h1 align="center">Noa</h1>

<p align="center"><strong>에이전트를 위한 네이티브 분산 버전 관리 시스템</strong></p>

<div align="center">

[![License](https://img.shields.io/badge/license-SySL%201.0-blue)](../../LICENSE)
[![GitHub](https://img.shields.io/badge/github-celestia--island%2Fnoa-blue.svg)](https://github.com/celestia-island/noa)

</div>

<div align="center">

[English](../en/intro.md) ·
[简体中文](../zh-Hans/intro.md) ·
[繁體中文](../zh-Hant/intro.md) ·
[日本語](../ja/intro.md) ·
**한국어** ·
[Français](../fr/intro.md) ·
[Español](../es/intro.md) ·
[Русский](../ru/intro.md) ·
[العربية](../ar/intro.md)

</div>

에이전트별로 격리된 워크스페이스, SHA-256 콘텐츠 주소 방식 스냅샷, 완전한 git 스마트 프로토콜 서버 구현(clone/fetch/push), 미러와 백플로우를 갖춘 자체 호스팅 포지, 라인 단위 3방향 머지와 정확한 리네임 감지, 다중 사용자 RBAC, CI 실행 기록 API — 모두 단일 바이너리 + 임베디드 스토리지에 내장되어 있습니다.

## 문서

한국어 문서 전체는 본 리포지토리의 `docs/ko/`에 있습니다(본 사이트는
[noa.docs.celestia.world](https://noa.docs.celestia.world)에서도 동기화되어 공개됩니다):

| 문서 | 내용 |
|---|---|
| [빠른 시작](./guide/getting-started.md) | 빌드, 서버 시작, 첫 clone/push |
| [아키텍처 개요](./guide/architecture.md) | 워크스페이스 / 스냅샷 / 참조 모델, 서버 구성 요소 |
| [미러와 백플로우](./guide/mirror.md) | 두 개의 레인, PR heads, 백플로우와 카나리아, 충실도 검증 |
| [권한과 사용자](./guide/rbac.md) | 역할, 토큰, REST와 git 양채널 권한 집행 |
| [git-http 프로토콜](./guide/git-http.md) | 스마트 프로토콜 엔드포인트, 푸시 시맨틱스, 프로토콜 협상 |
| [머지와 충돌](./guide/merge.md) | 라인 단위 diff3, 정확한 리네임, 폴백 시맨틱스 |
| [CI 실행 기록](./guide/ci-runs.md) | runs API, chest 연동 |
| [HTTP API 레퍼런스](./reference/http-api.md) | 모든 REST 엔드포인트 |
| [CLI 레퍼런스](./reference/cli.md) | 커맨드라인 전체 모습 |
| [배포](./reference/deploy.md) | systemd, 환경 변수, 배포 3종 세트 |

영어 버전은 `docs/en/`에 있습니다(간체 중국어에서 단계별로 동기화).

소스 코드: [noa](https://github.com/celestia-island/noa).
