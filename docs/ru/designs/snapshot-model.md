# Дизайн модели снимков

## Обзор

Снимок (Snapshot) — это неизменяемая, контентно-адресуемая запись полного
состояния файлового дерева рабочей области на определённый момент времени. Снимки образуют направленный
ациклический граф (DAG) через родительские ссылки.

## Структура снимка

```rust
pub struct SnapshotId(pub String);  // "noa_<12-char-base62>"

pub struct Snapshot {
    pub id: SnapshotId,
    pub tree_hash: String,           // SHA-256 корневого дерева
    pub parents: Vec<SnapshotId>,    // 0-N родительских снимков
    pub workspace: String,           // исходная рабочая область
    pub author: String,              // идентификатор агента или человека
    pub timestamp: u64,              // микросекунды с эпохи
    pub message: String,             // человекочитаемое описание
}
```

## Генерация ID

ID снимков используют 12-символьную строку base62 с префиксом `noa_`:

```
noa_3kF8x2mP9aB1
```

Генерация: `SHA256(tree_hash || parents || workspace || timestamp)[0..9]`
закодировано как base62. Это обеспечивает:
- 62^12 ≈ 3,2 × 10^21 возможных ID
- Вероятность коллизии практически нулевая
- Детерминированность: одинаковые входные данные → одинаковый ID (позволяет дедупликацию)

## DAG снимков

```mermaid
graph TD
    empty["noa_empty (sentinel)"]
    empty --> a["noa_abc123<br/>(workspace: default, 'init')"]
    empty --> merge["noa_mno345<br/>(merge of feature-1 and feature-2 into default)<br/>parents: [noa_abc123, noa_ghi789, noa_jkl012]"]

    a --> b["noa_def456<br/>(workspace: feature-1, 'add login')"]
    a --> c["noa_jkl012<br/>(workspace: feature-2, 'fix bug')"]

    b --> d["noa_ghi789<br/>(workspace: feature-1, 'add tests')"]
```

## Процесс создания снимка

```mermaid
flowchart TD
    A["1. AgentLog replay"] --> A1["Read all write/delete/rename ops for workspace"]
    A1 --> B["2. Tree construction"]
    B --> B1["Start from parent snapshot's tree"]
    B1 --> B2["Apply ops in sequence order"]
    B2 --> B3["Store resulting tree → ObjectStore"]
    B3 --> C["3. Snapshot creation"]
    C --> C1["Build Snapshot struct with tree hash"]
    C1 --> C2["Compute ID from content"]
    C2 --> C3["Store in SnapshotStore (redb table)"]
    C3 --> D["4. Workspace update"]
    D --> D1["CAS update workspace head to new snapshot ID"]
```

## Хранилище снимков

Снимки хранятся в таблице redb с ключом по ID:

```
Таблица: snapshots
  Ключ:   "noa_abc123" (SnapshotId как &str)
  Значение: msgpack(Snapshot) как &[u8]
```

## Алгоритм сравнения (Diff)

`diff_snapshots(base, other)` создаёт список изменений на уровне файлов:

```rust
pub struct FileDiff {
    pub path: String,
    pub kind: DiffKind, // Added, Removed, Modified
    pub old_blob: Option<String>,
    pub new_blob: Option<String>,
}
```

Алгоритм:
1. Загрузить корневые деревья для обоих снимков
2. Рекурсивно обойти оба дерева одновременно
3. Сравнить хеши блобов по каждому пути
4. Разные хеши → Modified; только в одном → Added/Removed

Временная сложность: O(n), где n = общее количество файлов в обоих деревьях.

## Сторожевой снимок

`noa_empty` — это зарезервированный ID снимка, представляющий пустое дерево. Все
новые репозитории начинаются с него как с базы. Он никогда явно не
хранится — менеджер рабочих областей распознаёт его как «снимков пока нет».

## Сравнение с коммитами Git

| Аспект | Снимок noa | Коммит Git |
|--------|-------------|------------|
| Формат ID | `noa_<base62>` | SHA-1 hex |
| Лимит родителей | Без ограничений (DAG слияния) | Обычно 1-2 |
| Формат дерева | MessagePack | Собственный бинарный |
| Временная метка | Микросекундная точность | Секундная точность + часовой пояс |
| Поле автора | ID агента или человек | имя + email |
| Неизменяемость | Обеспечивается хранилищем | Обеспечивается хешем |
| Подпись GPG | Не поддерживается | Поддерживается |
