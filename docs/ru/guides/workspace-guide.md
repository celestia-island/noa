# Руководство по рабочим областям

Рабочие области (Workspaces) — это изолированные рабочие контексты, аналогичные веткам Git. Каждая
рабочая область имеет собственный головной снимок и журнал агента.

## Создание рабочих областей

```bash
noa workspace create feature-1
noa workspace create agent-debug --agent bot-42
```

Флаг `--agent` связывает рабочую область с конкретным идентификатором агента.

## Переключение рабочих областей

```bash
noa workspace switch feature-1
noa status
# В рабочей области: feature-1 (head: noa_abc123)
```

## Список рабочих областей

```bash
noa workspace list
#   default             head: noa_abc123 base: noa_empty
# * feature-1           head: noa_def456 base: noa_abc123
```

Маркер `*` показывает активную рабочую область.

## Слияние рабочих областей

```bash
noa workspace switch default
noa workspace merge feature-1
# Слито feature-1 в default -> noa_ghi789
```

При обнаружении конфликтов:

```
Обнаружены конфликты:
  CONFLICT: src/main.rs
Слито feature-1 в default -> noa_ghi789
```

Стратегия разрешения по умолчанию — upstream-wins (theirs). Будущие версии
будут поддерживать ручное разрешение конфликтов.

## Удаление рабочих областей

```bash
noa workspace delete feature-1
# Удалена рабочая область 'feature-1'
```

Нельзя удалить активную рабочую область.

## Шаблон рабочего процесса

```mermaid
flowchart TD
    S1["1. noa workspace create feature-1"]
    S2["2. noa workspace switch feature-1"]
    S3["3. (agent writes files and creates snapshots)"]
    S4["4. noa workspace switch default"]
    S5["5. noa workspace merge feature-1"]
    S6["6. noa workspace delete feature-1"]
    S1 --> S2 --> S3 --> S4 --> S5 --> S6
```

## Шаблон мульти-агентной работы

Каждый агент получает собственную рабочую область:

```mermaid
graph TD
    A1["Agent-001"] --> W1["workspace agent-001<br/>agent-logs/agent-001.log"]
    A2["Agent-002"] --> W2["workspace agent-002<br/>agent-logs/agent-002.log"]
    AN["Agent-N"] --> WN["workspace agent-N<br/>agent-logs/agent-N.log"]
```

Каждая рабочая область имеет независимый журнал агента (`.noa/agent-logs/agent-001.log`),
что позволяет параллельные записи без блокировок. Шаг консолидации объединяет все журналы
по временной метке для создания единой истории.

> **Примечание**: redb использует эксклюзивную файловую блокировку, поэтому несколько CLI-процессов
> не могут открыть одну базу данных одновременно. Для настоящей многопроцессной
> параллельной работы используйте HTTP API noa-server.
