# 원격 저장소 가이드

## 개요

noa는 콘텐츠 주소 지정 객체를 분산하고 백업하기 위해 여러 원격 저장소 백엔드를 지원합니다. 백엔드는 저장소별로 구성되며 통합된 `noa storage` 명령으로 관리됩니다.

## 지원되는 백엔드

| 백엔드 | 타입 | 요구사항 | 사용 사례 |
|---------|------|----------|----------|
| IPFS (Kubo) | `ipfs` | 실행 중인 IPFS 데몬 | 분산 P2P 배포 |
| S3 / MinIO | `s3` | S3 호환 엔드포인트 | 중앙 집중식 백업, 클라우드 저장소 |

## 저장소 백엔드 추가

### IPFS

먼저 Kubo 데몬을 시작합니다:

```bash
ipfs daemon &   # 127.0.0.1:5001에서 수신
```

백엔드를 추가합니다:

```bash
# 기본값으로 IPFS 추가 (endpoint=http://127.0.0.1:5001, gateway=https://ipfs.io)
noa storage add ipfs-local --type ipfs

# 엔드포인트와 게이트웨이 사용자 지정
noa storage add ipfs-local --type ipfs \
  --endpoint http://192.168.1.100:5001 \
  --gateway https://dweb.link

# 원격 핀 서비스 사용 (예: Pinata)
noa storage add pinata --type ipfs \
  --endpoint https://api.pinata.cloud/psa \
  --auth-token YOUR_PINATA_TOKEN

# 매 푸시마다 자동 핀 활성화
noa storage add ipfs-local --type ipfs --auto-pin
```

### S3 / MinIO

```bash
# S3 호환 백엔드 추가
noa storage add s3-backup --type s3 \
  --endpoint https://s3.us-east-1.amazonaws.com \
  --bucket my-noa-objects \
  --access-key AKIAIOSFODNN7EXAMPLE \
  --secret-key wJalrXUtnFEMI/K7MDENG/bPxRfiCYEXAMPLEKEY \
  --region us-east-1

# 로컬 MinIO 서버 추가
noa storage add minio-local --type s3 \
  --endpoint http://localhost:9000 \
  --bucket noa \
  --access-key minioadmin \
  --secret-key minioadmin
```

## 백엔드 관리

```bash
# 구성된 모든 백엔드 나열
noa storage list
# ipfs-local    http://127.0.0.1:5001 (ipfs) [gateway=https://ipfs.io]
# s3-backup     https://s3.example.com (s3) [bucket=noa-objects]

# 연결 상태 확인
noa storage status               # 모든 백엔드
noa storage status ipfs-local    # 특정 백엔드

# 백엔드 제거
noa storage remove s3-backup
```

## 스냅샷 푸시

분산 또는 백업을 위해 객체를 원격 백엔드에 푸시합니다:

```bash
# 모든 스냅샷을 특정 백엔드에 푸시
noa storage push --target ipfs-local

# 푸시 및 핀 (IPFS 전용 — 가비지 컬렉션 방지)
noa storage push --target ipfs-local --pin

# 특정 스냅샷 푸시
noa storage push --target s3-backup --snapshot noa_abc123

# 워크스페이스의 모든 스냅샷 푸시
noa storage push --target ipfs-local --workspace feature-auth --pin
```

구성에 `auto_pin = true`가 있으면 `--pin`이 암시됩니다. `--target`을 생략하여 모든 자동 핀 백엔드에 한 번에 푸시할 수도 있습니다:

```bash
noa storage push --pin   # auto_pin=true인 모든 백엔드에 푸시
```

## 객체 가져오기

원격 백엔드에서 객체를 다운로드하여 로컬에 저장합니다:

```bash
# SHA-256 해시로 가져오기 (모든 백엔드)
noa storage fetch ipfs-local 2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824

# CID로 가져오기 (IPFS 전용)
noa storage fetch ipfs-local bafkreihdwdcefgh4dqkjv67uzcmw7ojel6viejvs6buyq7omaygs5sep7m
```

## 푸시 작동 방식

1. **로컬 우선**: noa는 로컬 `RedbObjectStore`에서 객체를 읽습니다
2. **재귀적 전송**: 각 스냅샷에 대해 전체 트리(blob과 하위 트리)를 순회합니다. 원격에 없는 객체가 전송됩니다.
3. **콘텐츠 주소 지정**: 두 백엔드 모두 SHA-256을 사용합니다. IPFS의 경우 해시를 CIDv1(raw 코덱)으로 변환합니다. S3의 경우 해시를 객체 키로 사용합니다.
4. **핀** (IPFS 전용): 푸시 후 `--pin`은 데몬에게 객체를 유지하도록 지시하여 가비지 컬렉션을 방지합니다.

## 구성 형식

```toml
# .noa/config

[[storage]]
name = "ipfs-local"
type = "ipfs"
endpoint = "http://127.0.0.1:5001"
gateway = "https://ipfs.io"
auto_pin = true

[[storage]]
name = "s3-backup"
type = "s3"
endpoint = "https://s3.example.com"
bucket = "noa-objects"
access_key = "AKIA..."
secret_key = "..."
region = "us-east-1"
```

## 프로그래밍 방식 사용

```rust
use libnoa::config::StorageConfig;
use libnoa::object::{create_remote_store, ObjectStore};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cfg = StorageConfig::ipfs("local", "http://127.0.0.1:5001");
    let store = create_remote_store(&cfg).await?;

    // 콘텐츠를 원격에 저장
    let blob_id = store.put_blob(b"hello world").await?;
    println!("Stored as: {}", blob_id);

    // 존재 여부 확인
    assert!(store.has_blob(&blob_id).await?);

    // 가져오기
    let data = store.get_blob(&blob_id).await?;
    assert_eq!(data, b"hello world");

    Ok(())
}
```
