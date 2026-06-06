# Справочник CLI

## `noa init [path]`

Инициализировать новый репозиторий `.noa/`. Создаёт `noa.redb`, `agent-logs/`, `HEAD` и `config`.

```bash
noa init .           # текущая директория
noa init /path/repo  # указанный путь
```

## `noa status`

Показать текущую рабочую область и головной снимок.

```bash
noa status
# В рабочей области: default (head: noa_abc123, msg: initial)
```

## `noa log [options]`

Просмотр истории снимков.

| Флаг | По умолчанию | Описание |
|------|---------|-------------|
| `-w, --workspace` | текущий HEAD | Фильтровать по рабочей области |
| `-l, --limit` | 20 | Максимальное количество записей для отображения |

```bash
noa log
noa log --workspace feature-1 --limit 50
```

## `noa snapshot <subcommand>`

### `noa snapshot create [-m msg] [-a author]`

Создать снимок из журнала агента текущей рабочей области.

```bash
noa snapshot create -m "добавить функцию входа" -a "agent-001"
```

### `noa snapshot list`

Список всех снимков по всем рабочим областям.

### `noa snapshot diff <a> <b>`

Показать различия на уровне файлов между двумя снимками.

```bash
noa snapshot diff noa_abc123 noa_def456
```

## `noa workspace <subcommand>`

### `noa workspace create <name> [--agent <id>]`

Создать новую рабочую область, ответвлённую от текущего HEAD.

### `noa workspace switch <name>`

Переключить активную рабочую область (обновляет HEAD).

### `noa workspace list`

Список всех рабочих областей. `*` отмечает активную.

### `noa workspace delete <name>`

Удалить рабочую область (нельзя удалить активную рабочую область).

### `noa workspace merge <from>`

Слить другую рабочую область в текущую с использованием трёхстороннего слияния.

```bash
noa workspace switch default
noa workspace merge feature-1
```

## `noa remote <subcommand>`

### `noa remote add <name> <url>`

Добавить удалённый репозиторий.

### `noa remote remove <name>`

Удалить удалённый репозиторий.

### `noa remote list`

Список всех настроенных удалённых репозиториев.

## `noa push [--remote name]`

Отправить в удалённый репозиторий (ещё не реализовано).

## `noa pull [--remote name]`

Получить из удалённого репозитория (ещё не реализовано).

## `noa fetch [--remote name]`

Получить из удалённого репозитория без слияния (ещё не реализовано).

## `noa clone <url> [path]`

Клонировать удалённый репозиторий (ещё не реализовано).
