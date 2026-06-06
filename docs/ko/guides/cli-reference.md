# CLI 레퍼런스

## `noa init [path]`

새 `.noa/` 저장소를 초기화합니다. `noa.redb`, `agent-logs/`, `HEAD`, `config`를 생성합니다.

```bash
noa init .           # 현재 디렉터리
noa init /path/repo  # 특정 경로
```

## `noa status`

현재 워크스페이스와 헤드 스냅샷을 표시합니다.

```bash
noa status
# 워크스페이스: default (head: noa_abc123, msg: initial)
```

## `noa log [options]`

스냅샷 히스토리를 봅니다.

| 플래그 | 기본값 | 설명 |
|------|---------|-------------|
| `-w, --workspace` | 현재 HEAD | 워크스페이스별 필터 |
| `-l, --limit` | 20 | 표시할 최대 항목 수 |

```bash
noa log
noa log --workspace feature-1 --limit 50
```

## `noa snapshot <subcommand>`

### `noa snapshot create [-m msg] [-a author]`

현재 워크스페이스의 에이전트 로그에서 스냅샷을 생성합니다.

```bash
noa snapshot create -m "로그인 기능 추가" -a "agent-001"
```

### `noa snapshot list`

모든 워크스페이스의 모든 스냅샷을 나열합니다.

### `noa snapshot diff <a> <b>`

두 스냅샷 간의 파일 수준 차이를 표시합니다.

```bash
noa snapshot diff noa_abc123 noa_def456
```

## `noa workspace <subcommand>`

### `noa workspace create <name> [--agent <id>]`

현재 HEAD에서 분기된 새 워크스페이스를 생성합니다.

### `noa workspace switch <name>`

활성 워크스페이스를 전환합니다 (HEAD 업데이트).

### `noa workspace list`

모든 워크스페이스를 나열합니다. `*`는 활성 워크스페이스를 표시합니다.

### `noa workspace delete <name>`

워크스페이스를 삭제합니다 (활성 워크스페이스는 삭제할 수 없습니다).

### `noa workspace merge <from>`

삼방향 병합을 사용하여 다른 워크스페이스를 현재 워크스페이스로 병합합니다.

```bash
noa workspace switch default
noa workspace merge feature-1
```

## `noa remote <subcommand>`

### `noa remote add <name> <url>`

원격 저장소를 추가합니다.

### `noa remote remove <name>`

원격 저장소를 제거합니다.

### `noa remote list`

설정된 모든 원격 저장소를 나열합니다.

## `noa push [--remote name]`

원격 저장소로 푸시합니다 (아직 구현되지 않음).

## `noa pull [--remote name]`

원격 저장소에서 풀합니다 (아직 구현되지 않음).

## `noa fetch [--remote name]`

병합 없이 원격 저장소에서 페치합니다 (아직 구현되지 않음).

## `noa clone <url> [path]`

원격 저장소를 클론합니다 (아직 구현되지 않음).
