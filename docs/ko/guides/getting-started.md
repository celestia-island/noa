# 시작하기

## 필수 조건

- Rust 1.75+ (stable)
- Python 3.8+ (빌드 스크립트용)
- `just` 명령어 러너

## 설치

```bash
git clone https://github.com/celestia-island/noa.git
cd noa
just init          # 의존성 가져오기
just build-dev     # 개발 빌드
```

`noa` 바이너리는 `target/debug/noa`에 있습니다.

## 빠른 시작

```bash
# 새 저장소 초기화
noa init .

# 상태 확인
noa status
# 워크스페이스: default

# 워크스페이스 생성
noa workspace create feature-1

# 워크스페이스로 전환
noa workspace switch feature-1

# 스냅샷 생성
noa snapshot create -m "초기 작업"

# 히스토리 보기
noa log

# 다시 전환하고 병합
noa workspace switch default
noa workspace merge feature-1

# 원격 저장소 관리
noa remote add origin https://github.com/example/repo.git
noa remote list
```

## 예제 실행

```bash
python3 examples/run_all.py
```

## 개발

```bash
just fmt            # 코드 포맷
just clippy         # 린트
just test           # 테스트 실행
just check          # 타입 검사
```
