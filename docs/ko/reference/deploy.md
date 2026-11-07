# 배포

noa-server의 배포 형태: 바이너리 하나 + redb 데이터베이스 하나 + systemd
유닛. 프로덕션 인스턴스는 malkuth가 관리합니다.

## 배포 3종 세트

1. **산출물**: `cargo build --release --locked --bin noa-server` 결과물을
   배포 경로로 복사합니다(원자적 교체: 먼저 `<bin>.new`로 쓰고 rename),
   양쪽에서 sha256을 검증합니다;
2. **health 재시도**: 시작 후 `/api/v1/*`를 폴링합니다(401 = 프로세스
   생존 + 인증 동작);
3. **롤백**: 이전 산출물 경로를 유지합니다; `mv`로 되돌리고 + 재시작합니다.

## 환경 변수 (A 클래스 부트스트랩 설정 → 0600 env 파일)

| 변수 | 설명 |
|---|---|
| `NOA_API_TOKEN` | 부트스트랩 관리자 토큰; **빈 값 / 공백뿐이면 시작 거부** |
| `NOA_MIRROR_ENABLED` | 미러 스케줄링 스위치 |
| `NOA_MIRROR_INTERVAL_SECS` | 웨이브 간격 (기본 900) |
| `NOA_MIRROR_SCRATCH_DIR` | 스크래치 루트 (예: `/var/lib/noa/mirror-cache`) |
| `NOA_MIRROR_GH_TOKEN` | 백플로우 / 카나리용 GitHub 쓰기 자격 증명 (기본 `token_env` 이름) |
| `NOA_MIRROR_GH_TOKEN_ENV` | 자격 증명을 담는 env 변수 이름을 재정의 (기본은 위 행) |

자격 증명은 배포 평면(0600 env / systemd LoadCredential)에만 존재하며,
DB, git 트리, 문서에는 절대 들어가지 않습니다.

## systemd 유닛

```ini
[Service]
EnvironmentFile=/etc/noa/noa-server.env
ExecStart=/usr/local/bin/noa-server --host 0.0.0.0 --port 3016
Restart=on-failure
```

- `0.0.0.0`으로 수신합니다(family-node 관례: 전체 라우트 베어러 인증 + 쓰기
  역할 강제, 안전한 노출 면);
- env 변경 후: `systemctl daemon-reload && systemctl restart`;
  `tr '\0' '\n' < /proc/$MAINPID/environ`에 새 키가 포함되는지 검증합니다;
- 프로덕션 인스턴스는 malkuth가 관리합니다(배포 경로를 만지기 전에 배포
  독점 클레임을 획득하세요).

## 포트와 도메인

포트 할당은 워크스페이스 장부(`_ledger/ports-and-domains.md`)에 있습니다;
프로덕션 인스턴스는 3016이며, 앞단 nginx가 TLS를 종료합니다.

## 다중 인스턴스

redb는 단일 파일 스토어입니다 — 인스턴스 하나, 데이터베이스 파일 하나.
다중 인스턴스 = 다중 디렉터리, 다중 포트(예: `chest-demo`와 프로덕션 chest의
듀얼 인스턴스 모드); 인스턴스 간 설정 전송은 RPC import / export로 하며,
설정 파일 복사로 하지 않습니다.

## 배포 후 검증

```bash
# Auth and readiness
curl -s -o /dev/null -w '%{http_code}\n' http://127.0.0.1:3016/api/v1/prs   # expect 401
# Mirror wave
curl -X POST -H "Authorization: Bearer $T" -H "Content-Type: application/json" \
  -d '{"job":"noa"}' http://127.0.0.1:3016/api/v1/mirror/run               # expect 202
```

미러 상세와 백플로우 시맨틱스:
[미러와 백플로우](../guide/mirror.md) 참조.
