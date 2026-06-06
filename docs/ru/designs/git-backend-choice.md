# Выбор Git-бэкенда: gix (gitoxide) vs git2 (libgit2)

## Статус: Анализ

**Текущий**: `git2 = "0.19"` (C-биндинг к libgit2)
**Предлагаемый**: `gix = "0.84"` (чистая Rust-реализация git)

## Резюме

gix (gitoxide) — это зрелая чистая Rust-реализация Git с достаточным
покрытием функциональности для замены git2 в git-мосте noa. Миграция
устраняет C-зависимость (libgit2), снижает сложность кросс-компиляции
и предоставляет идиоматичные Rust API.

## Матрица сравнения

| Критерий | git2 (libgit2) | gix (gitoxide) |
|-----------|---------------|----------------|
| **Язык** | C (Rust-биндинги через крейт git2) | Чистый Rust |
| **Зрелость** | 14 лет, проверено в продакшене | 5 лет, активная разработка (0.84) |
| **Компиляция** | ~15с (пересборка), требует CMake + libgit2-dev | ~8с (пересборка), только cargo |
| **Кросс-компиляция** | Сложно (нужен C-инструментарий) | Тривиально (cargo cross-compile) |
| **Стиль API** | C-подобный, unsafe-блоки, ручное управление временем жизни | Rust-идиоматичный, безопасные заимствования, паттерны builder |
| **Работа с объектами** | git2::Blob, Tree, Commit через ODB | gix::objs::BlobRef, TreeRef, CommitRef |
| **Обход дерева** | Ручной итератор с .to_object() | breadthfirst/virtual_roots с делегатом |
| **Удалённые push/pull** | git2::Remote (fetch, push) | gix::remote (connect, fetch, push) |
| **Pack/pack-index** | Встроенный | Комплексный (отдельный крейт: gix-pack) |
| **Refs** | git2::Reference (чтение/запись) | gix::refs (полная поддержка транзакций) |
| **Конфигурация** | Ограниченная (уровень репозитория) | Многослойная (системная, пользовательская, репозиторий) |
| **SHA-1/256** | Только SHA-1 | SHA-1 + SHA-256 (экспериментально) |
| **Безопасность памяти** | Риск из-за C-багов libgit2 | Гарантии Rust |
| **Аудируемость** | Нужно аудировать кодовую базу libgit2 на C | Только Rust, cargo-audit |
| **Сообщество** | Огромное (все основные VCS-инструменты) | Растущее (gitoxide, crates-index-diff и др.) |

## Требования git-моста noa

Текущее использование в `src/git/`:

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
//   - Чистые манипуляции на уровне байтов (без внешней git-зависимости)

// export.rs:
//   - В настоящее время todo!() — push будет использовать gix::remote::connect()
//   - Генерация pack-файлов через gix-pack (при необходимости)
```

Все 6 текущих вызовов API имеют прямые эквиваленты в gix.

## Покрытие функций gix для noa

| Требуемая функция | Поддержка git2 | Поддержка gix | Примечания |
|---------------|-------------|-------------|-------|
| Открыть репозиторий | ✅ | ✅ | `gix::open()` или `gix::ThreadSafeRepository::open()` |
| Прочитать HEAD ref | ✅ | ✅ | `gix.head_ref()` / `gix.head()` |
| Найти коммит по OID | ✅ | ✅ | `gix.find_object(id)?.try_into_commit()` |
| Прочитать дерево из коммита | ✅ | ✅ | `gix.find_object(commit.tree())?.try_into_tree()` |
| Итерировать записи дерева | ✅ | ✅ | `tree.iter()` возвращает `TreeRefIter` |
| Прочитать содержимое блоба | ✅ | ✅ | `blob.data` на `BlobRef` |
| Fetch из удалённого репозитория | ✅ | ✅ | `gix::remote::connect()`  |
| Push в удалённый репозиторий | ✅ | ✅ | `gix::remote::connect()`  |
| Clone | ✅ | ✅ | `gix::prepare_clone()` |
| Генерация pack-файлов | ✅ | ✅ | крейт `gix-pack` |
| Поддержка SHA-256 | ❌ | ✅ (экспериментально) | Актуально для SHA-256 снимков |
| Поддержка async | ❌ | ✅ (опционально) | Хорошо для интеграции с tokio |

## Осуществимость

Все текущие и планируемые git-операции имеют эквиваленты в gix. Отображение
API прямолинейно:

```rust
// git2 (текущий)
let repo = git2::Repository::open(path)?;
let head = repo.head()?;
let commit = repo.find_commit(head.target().unwrap())?;
let tree = commit.tree()?;

// gix (предлагаемый)
let repo = gix::open(path)?;
let head = repo.head_ref()?.expect("HEAD not found");
let head_id = head.id().detach();
let commit = repo.find_object(head_id)?.try_into_commit()
    .map_err(|_| NoaError::Remote("not a commit".into()))?;
let tree = repo.find_object(commit.tree())?.try_into_tree()
    .map_err(|_| NoaError::Remote("not a tree".into()))?;
```

## План миграции

### Фаза 1: Замена import.rs (операции только для чтения)
- Заменить git2::Repository на gix::ThreadSafeRepository
- Перереализовать обход дерева
- Запустить существующие тесты импорта git

### Фаза 2: Замена translate.rs
- Изменения не требуются (чистые манипуляции с байтами, без C-зависимости)

### Фаза 3: Реализация export.rs через gix
- Использовать gix::remote для push
- Использовать gix::prepare_clone для clone
- Использовать gix-pack для генерации pack-файлов (при необходимости для серверной стороны)

### Фаза 4: Удаление git2 из Cargo.toml
- Убрать системную зависимость libgit2
- Проверить кросс-компиляцию (x86_64 → aarch64, → wasm в будущем)

## Оценка рисков

| Риск | Вероятность | Влияние | Смягчение |
|------|-----------|--------|------------|
| Поломка API gix (0.x) | Средняя | Низкое | Зафиксировать версию, адаптироваться к изменениям API |
| Отсутствие продвинутых функций | Низкая | Среднее | gix имеет удалённые push/fetch с версии 0.50+ |
| Регрессия производительности | Низкая | Низкое | gix часто быстрее (нет накладных расходов C FFI) |
| Риск принятия сообществом | Низкий | Низкое | gix — де-факто стандартная Rust-библиотека git |
| Баги совместимости SHA-256 | Средняя | Низкое | Скрыто за feature-флагом, обход через чистый translate.rs |

## Рекомендация

**Мигрировать на gix.** Преимущества (нулевые C-зависимости, чистая Rust-безопасность,
более простая кросс-компиляция, поддержка SHA-256) перевешивают риски (стабильность
API 0.x, меньшее сообщество). Миграция имеет низкий риск, потому что:

1. Текущее использование git2 минимально (6 вызовов API в import.rs)
2. translate.rs не требует изменений
3. export.rs не реализован (чистое поле для gix)
4. gix — стандартная Rust-библиотека git (используется индексом crates.io)

## Зависимости после миграции

```diff
- git2 = "0.19"           # C-биндинг libgit2
+ gix = { version = "0.84", features = ["basic", "index", "pack"] }
```

Никаких новых системных зависимостей. Чистый `cargo build`.
