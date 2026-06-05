# Git 백엔드 선택: gix (gitoxide) vs git2 (libgit2)

## 상태: 분석

**현재**: `git2 = "0.19"` (libgit2에 대한 C 바인딩)
**제안**: `gix = "0.84"` (순수 Rust git 구현)

## 요약

gix(gitoxide)는 noa의 git 브리지에서 git2를 대체하기에 충분한 기능 범위를 갖춘 성숙한 순수 Rust Git 구현입니다. 이 마이그레이션은 C 의존성(libgit2)을 제거하고, 교차 컴파일 마찰을 줄이며, 관용적인 Rust API를 제공합니다.

## 비교 매트릭스

| 기준 | git2 (libgit2) | gix (gitoxide) |
|-----------|---------------|----------------|
| **언어** | C (git2 크레이트를 통한 Rust 바인딩) | 순수 Rust |
| **성숙도** | 14년, 프로덕션 검증됨 | 5년, 활발한 개발 중 (0.84) |
| **컴파일** | ~15초 (재빌드), CMake + libgit2-dev 필요 | ~8초 (재빌드), cargo 전용 |
| **교차 컴파일** | 까다로움 (C 크로스 툴체인 필요) | 간단함 (cargo cross-compile) |
| **API 스타일** | C 스타일, unsafe 블록, 수동 수명 | Rust 관용적, borrow-safe, 빌더 패턴 |
| **객체 처리** | ODB를 통한 git2::Blob, Tree, Commit | gix::objs::BlobRef, TreeRef, CommitRef |
| **트리 순회** | .to_object()를 사용한 수동 이터레이터 | delegate를 사용한 breadthfirst/virtual_roots |
| **원격 push/pull** | git2::Remote (fetch, push) | gix::remote (connect, fetch, push) |
| **Pack/pack-index** | 내장 | 포괄적 (자체 크레이트: gix-pack) |
| **Refs** | git2::Reference (읽기/쓰기) | gix::refs (전체 트랜잭션 지원) |
| **Config** | 제한적 (저장소 수준) | 계층형 config (시스템, 사용자, 저장소) |
| **SHA-1/256** | SHA-1 전용 | SHA-1 + SHA-256 (실험적) |
| **메모리 안전성** | libgit2 C 버그 위험 | Rust 보장 |
| **감사 가능성** | libgit2 C 코드베이스 감사 필요 | Rust 전용, cargo-audit |
| **커뮤니티** | 거대함 (모든 주요 VCS 도구) | 성장 중 (gitoxide, crates-index-diff 등) |

## noa의 Git 브리지 요구사항

`src/git/`에서의 현재 사용:

```rust
// import.rs:
//   - Repository::open()           → gix::open()
//   - repo.head().target()         → gix.head().project_id()
//   - repo.find_commit(oid)        → gix.find_object().try_into_commit()
//   - commit.tree()                → gix.find_object(commit.tree()).try_into_tree()
//   - tree.iter()                  → gix::objs::TreeRefIter
//   - entry.to_object(repo)        → gix.find_object(entry.oid())
//   - obj.kind() === Blob          → obj.kind == ObjectKind::Blob
//   - blob.content()               → blob.data

// translate.rs:
//   - 순수 바이트 수준 조작 (외부 git 의존성 없음)

// export.rs:
//   - 현재 todo!() — push는 gix::remote::connect()를 사용할 예정
//   - gix-pack을 통한 팩 파일 생성 (필요한 경우)
```

현재 6개의 API 호출 모두 직접적인 gix 대응 항목이 있습니다.

## noa에 필요한 gix 기능 범위

| 필요한 기능 | git2 지원 | gix 지원 | 비고 |
|---------------|-------------|-------------|-------|
| 저장소 열기 | ✅ | ✅ | `gix::open()` 또는 `gix::ThreadSafeRepository::open()` |
| HEAD ref 읽기 | ✅ | ✅ | `gix.head_ref()` / `gix.head()` |
| OID로 커밋 찾기 | ✅ | ✅ | `gix.find_object(id)?.try_into_commit()` |
| 커밋에서 트리 읽기 | ✅ | ✅ | `gix.find_object(commit.tree())?.try_into_tree()` |
| 트리 항목 순회 | ✅ | ✅ | `tree.iter()`가 `TreeRefIter` 반환 |
| blob 콘텐츠 읽기 | ✅ | ✅ | `BlobRef`의 `blob.data` |
| 원격에서 fetch | ✅ | ✅ | `gix::remote::connect()` |
| 원격에 push | ✅ | ✅ | `gix::remote::connect()` |
| Clone | ✅ | ✅ | `gix::prepare_clone()` |
| 팩 파일 생성 | ✅ | ✅ | `gix-pack` 크레이트 |
| SHA-256 지원 | ❌ | ✅ (실험적) | SHA-256 스냅샷 관련 |
| 비동기 지원 | ❌ | ✅ (선택적) | tokio 통합에 유용 |

## 실현 가능성

현재 및 계획된 모든 git 작업에는 gix 대응 항목이 있습니다. API 매핑은 직관적입니다:

```rust
// git2 (현재)
let repo = git2::Repository::open(path)?;
let head = repo.head()?;
let commit = repo.find_commit(head.target().unwrap())?;
let tree = commit.tree()?;

// gix (제안)
let repo = gix::open(path)?;
let head = repo.head_ref()?.expect("HEAD not found");
let head_id = head.id().detach();
let commit = repo.find_object(head_id)?.try_into_commit()
    .map_err(|_| NoaError::Remote("not a commit".into()))?;
let tree = repo.find_object(commit.tree())?.try_into_tree()
    .map_err(|_| NoaError::Remote("not a tree".into()))?;
```

## 마이그레이션 계획

### 1단계: import.rs 교체 (읽기 전용 작업)
- git2::Repository를 gix::ThreadSafeRepository로 교체
- 트리 순회 재구현
- 기존 git import 테스트 실행

### 2단계: translate.rs 교체
- 변경 불필요 (순수 바이트 조작, C 의존성 없음)

### 3단계: gix를 통한 export.rs 구현
- push에 gix::remote 사용
- clone에 gix::prepare_clone 사용
- 팩파일 생성에 gix-pack 사용 (서버 측에서 필요한 경우)

### 4단계: Cargo.toml에서 git2 제거
- libgit2 시스템 의존성 제거
- 교차 컴파일 검증 (x86_64 → aarch64, 향후 → wasm)

## 위험 평가

| 위험 | 가능성 | 영향 | 완화 방안 |
|------|-----------|--------|------------|
| gix API 중단 (0.x) | 중간 | 낮음 | 버전 고정, API 변경에 적응 |
| 고급 기능 누락 | 낮음 | 중간 | gix는 0.50+부터 원격 push/fetch 지원 |
| 성능 회귀 | 낮음 | 낮음 | gix가 종종 더 빠름 (C FFI 오버헤드 없음) |
| 커뮤니티 채택 위험 | 낮음 | 낮음 | gix는 사실상의 Rust git 라이브러리 |
| SHA-256 상호 운용 버그 | 중간 | 낮음 | 기능 게이트, 순수 translate.rs로 우회 |

## 권장 사항

**gix로 마이그레이션.** 이점(C 의존성 제로, 순수 Rust 안전성, 더 쉬운 교차 컴파일, SHA-256 지원)이 위험(0.x API 안정성, 작은 커뮤니티)을 능가합니다. 다음과 같은 이유로 마이그레이션은 저위험입니다:

1. 현재 git2 사용은 최소한입니다 (import.rs에서 6개의 API 호출)
2. translate.rs는 변경이 필요 없습니다
3. export.rs는 미구현 상태입니다 (gix를 위한 신규 개발)
4. gix는 표준 Rust git 라이브러리입니다 (crates.io 인덱스에서 사용)

## 마이그레이션 후 의존성

```diff
- git2 = "0.19"           # libgit2 C 바인딩
+ gix = { version = "0.84", features = ["basic", "index", "pack"] }
```

새로운 시스템 의존성이 없습니다. 순수 `cargo build`.
