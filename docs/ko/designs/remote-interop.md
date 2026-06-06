# 원격 상호 운용성 설계

## 개요

noa는 여러 머신과 팀 간에 스냅샷과 객체를 동기화하기 위한 다양한 원격 백엔드를 지원합니다. 주요 상호 운용성 대상은 Git으로, GitHub, GitLab, Bitbucket의 기존 워크플로우와의 원활한 통합을 가능하게 합니다.

## 원격 백엔드 트레이트

```rust
#[async_trait]
pub trait RemoteBackend: Send + Sync {
    async fn push_snapshots(&self, ids: &[SnapshotId]) -> Result<()>;
    async fn fetch_snapshots(&self, ids: &[SnapshotId]) -> Result<Vec<Snapshot>>;
    async fn push_objects(&self, ids: &[String]) -> Result<()>;
    async fn fetch_objects(&self, ids: &[String]) -> Result<()>;
    async fn list_refs(&self) -> Result<HashMap<String, SnapshotId>>;
    async fn update_ref(&self, name: &str, old: Option<&SnapshotId>, new: &SnapshotId) -> Result<()>;
}
```

## Git 변환 계층

`GitTranslator`는 noa의 객체 모델과 Git의 객체 모델 간에 변환합니다:

### Blob ↔ Git Blob

```mermaid
graph LR
    subgraph Noa
        NB["noa blob:<br/>원시 바이트<br/>SHA-256 해시"]
    end
    subgraph Git
        GB["Git blob:<br/>'blob &lt;size&gt;\\0&lt;content&gt;'<br/>SHA-1 해시"]
    end
    NB -- "Git의 blob 헤더 형식으로<br/>콘텐츠 재해시" --> GB
```

### Tree ↔ Git Tree

```mermaid
graph LR
    subgraph Noa
        NT["noa tree:<br/>MessagePack [{name, kind, hash}]"]
    end
    subgraph Git
        GT["Git tree:<br/>'&lt;mode&gt; &lt;name&gt;\\0&lt;20-byte-sha1&gt;' 항목"]
    end
    NT -- "TreeEntry::Blob → mode 100644<br/>TreeEntry::Tree → mode 040000<br/>SHA-256 → SHA-1 재해시" --> GT
```

### Snapshot ↔ Git Commit

```mermaid
graph LR
    subgraph "noa 스냅샷"
        NS["id: noa_abc123<br/>tree_hash: SHA-256<br/>parents: [noa_...]<br/>author: agent-001<br/>timestamp: 1717592400000000 (µs)<br/>message: '기능 추가'"]
    end
    subgraph "Git 커밋"
        GC["tree: SHA-1<br/>parent: SHA-1<br/>author: agent-001 &lt;agent@noa&gt;<br/>message: '기능 추가'"]
    end
    NS -- "tree_hash 재해시 (SHA-256 → SHA-1)<br/>parents는 ID 조회로 매핑<br/>author는 더미 이메일로 형식화<br/>µs 타임스탬프는 초 단위로 절삭" --> GC
```

### Workspace ↔ Git Branch

```mermaid
graph LR
    subgraph Noa
        NW["워크스페이스 'feature-1'<br/>(head: noa_abc123)"]
        ND["워크스페이스 'default'<br/>(head: noa_def456)"]
    end
    subgraph Git
        GB1["브랜치 'feature-1'<br/>(HEAD: git-sha1)"]
        GB2["브랜치 'main'<br/>(HEAD: git-sha1)"]
    end
    NW --> GB1
    ND --> GB2
```

### Ref 매핑

```mermaid
graph LR
    subgraph "noa Refs"
        NH["HEAD → default"]
        ND2["default → noa_abc"]
        NF["feature-1 → noa_def"]
    end
    subgraph "Git Refs"
        GH["HEAD → refs/heads/main"]
        GMAIN["refs/heads/main → git-sha1"]
        GF1["refs/heads/feature-1 → git-sha2"]
    end
    NH -.-> GH
    ND2 -.-> GMAIN
    NF -.-> GF1
```

## Push 흐름

```mermaid
flowchart TD
    A["1. noa push --remote origin"] --> B["2. 워크스페이스 헤드에서 도달 가능한 모든 스냅샷 로드"]
    B --> C["3. 각 스냅샷 → Git 커밋으로 변환"]
    C --> D["4. 각 blob/tree → Git 객체로 변환"]
    D --> E["5. gix(gitoxide)를 통해 origin URL로 push"]
    E --> F["6. 원격 refs 업데이트"]
```

## Pull 흐름

```mermaid
flowchart TD
    A["1. noa pull --remote origin"] --> B["2. gix를 통해 refs fetch"]
    B --> C["3. 각 새로운 Git 커밋에 대해:"]
    C --> D["a. noa 스냅샷으로 변환<br/>b. blobs/trees를 noa 객체로 변환<br/>c. 로컬 redb에 저장"]
    D --> E["4. 병합 스냅샷 생성 (로컬 head + 원격 head)"]
    E --> F["5. 워크스페이스 head 업데이트"]
```

## MinIO/S3 백엔드

Git 인프라가 없는 배포의 경우:

```mermaid
flowchart TD
    A["noa push --remote s3-remote"] --> B["PUT /bucket/snapshots/noa_abc123 (msgpack)"]
    A --> C["PUT /bucket/blobs/&lt;sha256&gt; (원시 바이트)"]
    A --> D["PUT /bucket/trees/&lt;sha256&gt; (msgpack)"]
    A --> E["PUT /bucket/refs/default (스냅샷 ID 텍스트)"]
```

Git 원격 대비 장점:
- 트리/스냅샷 변환 불필요 (네이티브 noa 형식)
- 직접 blob 저장 (팩 파일 오버헤드 없음)
- S3 호환 (AWS, GCS, MinIO, Cloudflare R2와 작동)

## 원격 설정

`.noa/config`에 저장됨 (TOML):

```toml
[[remotes]]
name = "origin"
url = "https://github.com/example/repo.git"
backend = "git"

[[remotes]]
name = "s3"
url = "s3://my-bucket/noa-repo"
backend = "minio"
endpoint = "https://s3.amazonaws.com"
region = "us-east-1"
```

## 인증

| 백엔드 | 방법 |
|---------|--------|
| Git HTTPS | `~/.git-credentials`의 자격 증명 또는 프롬프트 |
| Git SSH | SSH 에이전트 또는 키 파일 |
| MinIO/S3 | 액세스 키 + 비밀 키 (환경 변수 또는 설정) |

## 비교: 원격 상호 운용 접근 방식

| 접근 방식 | 사용처 | 장점 | 단점 |
|----------|---------|------|------|
| Git 브리지 (gix) | noa | 보편적 호환성 | 변환 오버헤드, SHA-1/SHA-256 불일치 |
| 네이티브 프로토콜 | Git | 빠름, 변환 없음 | Git에서만 작동 |
| WebDAV | SVN | HTTP 표준 | 제한적, SVN 전용 |
| REST API | Bitbucket | 현대적, 유연함 | 호스팅 서비스 필요 |
| S3 호환 저장소 | noa | 확장 가능, 클라우드 네이티브 | 브리지 없이는 Git 상호 운용 불가 |

noa는 Git 브리지(호환성용)와 네이티브 S3(확장성용)를 모두 지원합니다.
