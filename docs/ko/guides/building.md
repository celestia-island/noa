# noa 빌드하기

## 필수 조건

- Rust 1.75+ (stable)
- Python 3.8+ (빌드 스크립트용)
- `just` 명령어 러너

## 설정

```bash
git clone https://github.com/celestia-island/noa.git
cd noa
just init          # Rust 의존성 가져오기
just build-dev     # 개발 빌드
```

## 개발

```bash
just fmt            # 코드 포맷
just clippy         # 린트
just test           # 테스트 실행
just check          # 타입 검사
```

## 프로젝트 구조

```mermaid
graph TD
    SRC["src/"] --> LIB["lib.rs<br/>(라이브러리 루트)"]
    SRC --> ERR["error.rs<br/>(에러 타입)"]
    SRC --> CFG["config.rs<br/>(설정)"]
    SRC --> REPO["repo.rs<br/>(저장소 생명주기)"]
    SRC --> OBJ["object/<br/>(ObjectStore 트레이트 + 구현체)"]
    SRC --> LOG["log/<br/>(AgentLog 트레이트 + 구현체)"]
    SRC --> SNAP["snapshot/<br/>(스냅샷 엔진)"]
    SRC --> WS["workspace/<br/>(워크스페이스 관리자)"]
    SRC --> REFS["refs.rs<br/>(RefStore 트레이트 + 구현체)"]
    SRC --> MERGE["merge/<br/>(병합 엔진)"]
    SRC --> GIT["git/<br/>(Git 호환성)"]
    SRC --> REMOTE["remote.rs<br/>(RemoteBackend 트레이트)"]
    SRC --> CLI["cli/<br/>(CLI 명령어)"]
```
