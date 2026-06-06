# 객체 저장소 설계

## 개요

noa는 Git에서 영감을 받았지만 교체 가능한 백엔드 아키텍처를 갖춘 콘텐츠 주소 지정 저장 모델을 사용합니다. 객체는 SHA-256 해시로 주소 지정되며 불투명한 blob으로 저장됩니다.

## 객체 유형

### Blob

원시 파일 콘텐츠. `SHA256(content)`로 식별됩니다.

```rust
pub struct BlobId(pub String); // 16진수 인코딩된 SHA-256
```

델타 압축이 없습니다. 각 고유 콘텐츠는 정확히 하나의 blob을 생성합니다.
중복 콘텐츠는 해시에 의해 자동으로 중복 제거됩니다.

### Tree

디렉터리 목록. 경로를 자식 항목(blob 또는 하위 트리)에 매핑합니다.

```rust
pub struct TreeEntry {
    pub name: String,
    pub kind: TreeEntryKind, // Blob 또는 Tree
    pub hash: String,        // 자식의 SHA-256
}

pub struct TreeId(pub String); // SHA-256(msgpack(entries))
```

트리는 간결성과 빠른 역직렬화를 위해 MessagePack으로 직렬화됩니다.

## 트레이트 정의

```rust
#[async_trait]
pub trait ObjectStore: Send + Sync {
    async fn put_blob(&self, data: &[u8]) -> Result<BlobId>;
    async fn get_blob(&self, id: &BlobId) -> Result<Vec<u8>>;
    async fn put_tree(&self, entries: Vec<TreeEntry>) -> Result<TreeId>;
    async fn get_tree(&self, id: &TreeId) -> Result<Vec<TreeEntry>>;
}
```

## 백엔드

### RedbObjectStore (로컬)

[redb](https://github.com/cberner/redb) 내장 키-값 저장소를 사용합니다.

- 두 개의 테이블: `blobs` (키: 해시 바이트, 값: 콘텐츠 바이트) 및
  `trees` (키: 해시 바이트, 값: msgpack 항목)
- 메모리 매핑 파일을 통한 제로 카피 읽기
- 자동 충돌 복구가 포함된 ACID 트랜잭션
- MVCC를 통한 단일 쓰기, 다중 읽기
- 외부 데몬 불필요

### MinioObjectStore (원격)

`aws-sdk-s3`를 통해 S3 호환 API를 사용합니다.

- 경로 스타일 주소 지정: `<bucket>/blobs/<hash>`, `<bucket>/trees/<hash>`
- 모든 S3 호환 백엔드 지원 (MinIO, AWS S3, GCS 등)
- 지수 백오프를 통한 자동 재시도
- 분산 배포에 적합

## 설계 결정

### SHA-1 대신 SHA-256을 사용하는 이유?

Git은 SHA-1을 사용하는데, 이는 암호학적으로 취약합니다 (SHAttered 공격, 2017).
SHA-256은 충돌 저항성이 있으며 널리 사용 가능합니다.

### 델타 압축을 사용하지 않는 이유?

1. **단순성**: 델타 압축(Git의 팩 파일)은 상당한 복잡성을 추가합니다
   (슬라이딩 윈도우 매칭, 씬 팩, 델타 체인).
2. **쓰기 성능**: 직접 blob 쓰기는 O(1)입니다. 델타 압축은
   기존 객체 읽기가 필요합니다.
3. **AI 에이전트 워크로드**: 에이전트는 전체 파일을 자주 재생성합니다.
   이전 버전은 일시적입니다 — 델타 체인은 짧고 많을 것입니다.
4. **백엔드 오프로딩**: S3/MinIO는 저장소 계층에서 중복 제거를 처리합니다.

### 트리에 MessagePack을 사용하는 이유?

- 바이너리 중심 데이터에 대해 JSON보다 30-50% 작음
- 스키마 유연성 (protobuf 정의 불필요)
- `rmp-serde`를 통한 Rust 생태계 지원
- 빠른 역직렬화

### SQLite 대신 redb를 사용하는 이유?

- **타입 안전성**: redb는 테이블 정의에 Rust 제네릭을 사용합니다
- **성능**: redb는 Rust 워크로드에 최적화됨 (제로 카피 읽기)
- **단순성**: 단일 의존성, C 라이브러리 연결 없음
- **충돌 안전성**: redb의 쓰기 선행 로그가 SQLite의 WAL 모드보다 단순함

트레이드오프: redb는 SQLite보다 커뮤니티가 작고 도구 옵션이 적습니다.
noa의 사용 사례(내장 바이너리 저장소)에서는 이 트레이드오프가 유리합니다.
