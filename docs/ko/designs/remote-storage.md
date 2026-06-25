# 원격 저장소 백엔드 설계

## 개요

noa는 콘텐츠 주소 지정 객체를 분산하고 백업하기 위해 플러그인 가능한 원격 저장소 백엔드를 지원합니다. 모든 백엔드는 동일한 `ObjectStore` 트레이트를 구현하므로, 스냅샷, 트리, blob을 구성된 백엔드 어디로든 교환 가능하게 푸시할 수 있습니다.

## 지원되는 백엔드

| 백엔드 | 타입 식별자 | 전송 방식 | 분산 모델 |
|---------|----------------|-----------|-------------------|
| Redb (로컬) | — (항상 로컬) | 내장 KV | 없음 |
| IPFS (Kubo) | `ipfs` | HTTP API | 피어 투 피어 (DHT, Bitswap) |
| S3 / MinIO | `s3` | S3 호환 API | 중앙 집중식 객체 저장소 |

## 구성

원격 백엔드는 `.noa/config`에 `[[storage]]` 배열로 저장됩니다:

```toml
[[storage]]
name = "ipfs-local"
type = "ipfs"
endpoint = "http://127.0.0.1:5001"
gateway = "https://ipfs.io"
auto_pin = false

[[storage]]
name = "s3-backup"
type = "s3"
endpoint = "https://s3.example.com"
bucket = "noa-objects"
access_key = "AKIA..."
secret_key = "..."
region = "us-east-1"
```

각 항목은 CLI 참조용 `name`, `type` 구분자, 백엔드별 필드를 가집니다. 주어진 타입에 대해 알 수 없는 필드는 무시됩니다.

## 팩토리 패턴

`create_remote_store(&StorageConfig) -> Box<dyn ObjectStore>`는 `backend_type` 필드를 검사하여 적절한 구현을 생성합니다:

```
type = "ipfs"  →  IpfsObjectStore  (reqwest HTTP 클라이언트 → Kubo 데몬)
type = "s3"    →  MinioObjectStore (aws-sdk-s3 → S3 호환 엔드포인트)
```

## IPFS CID 브리지

noa는 16진수로 인코딩된 SHA-256 해시(`BlobId`, `TreeId`)로 객체를 식별합니다. IPFS의 경우 이를 API 호출을 위해 CIDv1(raw 코덱)로 변환합니다:

```
CIDv1 바이트 = [0x01]           // 버전 1
               [0x55]           // raw 코덱
               [0x12]           // sha2-256 해시 함수
               [0x20]           // 32바이트 다이제스트 길이
               [32바이트 해시]

CIDv1 문자열 = "b" + base32_lowercase_nopad(CIDv1 바이트)
```

이 변환은 순수 함수입니다 — 동일한 콘텐츠는 항상 동일한 CID에 매핑됩니다. 매핑을 위해 데몬 왕복이 필요하지 않습니다.

## 라이브러리 선택: ipfs-api-backend-hyper 대신 reqwest

IPFS 백엔드는 `ipfs-api-backend-hyper` 크레이트 대신 `reqwest`를 직접 사용하여 Kubo HTTP API에 접근합니다. 이유:

- `aws-sdk-s3`(이미 의존성임)는 내부적으로 hyper를 사용합니다. `ipfs-api-backend-hyper`를 추가하면 hyper 버전 충돌 위험이 있습니다
- Kubo API는 얇은 REST 호출을 하기에 충분히 단순합니다
- `rustls-tls`와 함께 `reqwest`를 사용하면 OpenSSL 시스템 의존성을 피할 수 있습니다

## 푸시 전략

스냅샷을 푸시할 때 noa는 트리를 재귀적으로 순회합니다:

1. 트리의 각 blob에 대해: 원격에 존재하는지 확인 → 존재하지 않으면 푸시
2. 각 하위 트리에 대해: 재귀
3. 루트 트리를 푸시
4. `--pin`이 포함된 IPFS의 경우: 가비지 컬렉션을 방지하기 위해 루트 CID를 고정합니다

이를 통해 전체 스냅샷 그래프가 전송됩니다. 로컬 `RedbObjectStore`가 항상 진실의 원천이며, 원격 백엔드는 분산/백업 대상입니다.

## 오류 처리

백엔드별 오류는 `NoaError` 변형에 매핑됩니다:

- `IpfsDaemonUnreachable { endpoint }` — 연결 거부, 시간 초과
- `IpfsError { message }` — API 오류 응답
- `InvalidCid { cid }` — SHA-256 → CID 변환 실패
- `ObjectNotFound { id }` — 네트워크/저장소에서 블록을 찾을 수 없음

## 설계 결정

### 태그된 열거형 대신 평평한 구성체를 사용하는 이유는?

TOML은 네이티브 열거형을 지원하지 않습니다. `type` 구분자 필드와 선택적 백엔드별 필드를 가진 평평한 구성체가 가장 TOML 친화적인 접근 방식이며, 기존 `RemoteConfig` 패턴(`name` + `url` + `protocol`)과 일치합니다.

### `[[remotes]]`와 병합하지 않는 이유는?

Git 원격(`RemoteConfig`)과 객체 저장소(`StorageConfig`)는 서로 다른 목적을 제공합니다:
- **원격**은 git 프로토콜 푸시/풀에 사용됩니다 (소스 코드 분산)
- **저장소**는 콘텐츠 주소 지정 객체 분산에 사용됩니다 (스냅샷, blob)

이를 분리하여 유지하면 혼란을 방지하고 독립적인 구성이 가능합니다.
