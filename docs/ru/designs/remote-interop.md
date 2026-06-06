# Дизайн удалённой совместимости

## Обзор

noa поддерживает несколько удалённых бэкендов для синхронизации снимков и объектов
между машинами и командами. Основная цель совместимости — Git,
обеспечивающая бесшовную интеграцию с существующими рабочими процессами на GitHub, GitLab
и Bitbucket.

## Трейт удалённого бэкенда

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

## Слой трансляции Git

`GitTranslator` преобразует между объектной моделью noa и Git:

### Блоб ↔ Git Blob

```mermaid
graph LR
    subgraph Noa
        NB["noa blob:<br/>raw bytes<br/>SHA-256 hash"]
    end
    subgraph Git
        GB["Git blob:<br/>'blob &lt;size&gt;\\0&lt;content&gt;'<br/>SHA-1 hash"]
    end
    NB -- "rehash content with<br/>Git's blob header format" --> GB
```

### Дерево ↔ Git Tree

```mermaid
graph LR
    subgraph Noa
        NT["noa tree:<br/>MessagePack [{name, kind, hash}]"]
    end
    subgraph Git
        GT["Git tree:<br/>'&lt;mode&gt; &lt;name&gt;\\0&lt;20-byte-sha1&gt;' entries"]
    end
    NT -- "TreeEntry::Blob → mode 100644<br/>TreeEntry::Tree → mode 040000<br/>SHA-256 → SHA-1 rehash" --> GT
```

### Снимок ↔ Git Commit

```mermaid
graph LR
    subgraph "noa Snapshot"
        NS["id: noa_abc123<br/>tree_hash: SHA-256<br/>parents: [noa_...]<br/>author: agent-001<br/>timestamp: 1717592400000000 (µs)<br/>message: 'add feature'"]
    end
    subgraph "Git Commit"
        GC["tree: SHA-1<br/>parent: SHA-1<br/>author: agent-001 &lt;agent@noa&gt;<br/>message: 'add feature'"]
    end
    NS -- "tree_hash rehashed (SHA-256 → SHA-1)<br/>parents mapped via ID lookup<br/>author formatted with dummy email<br/>µs timestamp truncated to seconds" --> GC
```

### Рабочая область ↔ Ветка Git

```mermaid
graph LR
    subgraph Noa
        NW["workspace 'feature-1'<br/>(head: noa_abc123)"]
        ND["workspace 'default'<br/>(head: noa_def456)"]
    end
    subgraph Git
        GB1["branch 'feature-1'<br/>(HEAD: git-sha1)"]
        GB2["branch 'main'<br/>(HEAD: git-sha1)"]
    end
    NW --> GB1
    ND --> GB2
```

### Сопоставление ссылок (Ref)

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

## Процесс Push

```mermaid
flowchart TD
    A["1. noa push --remote origin"] --> B["2. Load all snapshots reachable from workspace heads"]
    B --> C["3. Translate each snapshot → Git commit"]
    C --> D["4. Translate each blob/tree → Git object"]
    D --> E["5. Push via gix (gitoxide) to origin URL"]
    E --> F["6. Update remote refs"]
```

## Процесс Pull

```mermaid
flowchart TD
    A["1. noa pull --remote origin"] --> B["2. Fetch refs via gix"]
    B --> C["3. For each new Git commit:"]
    C --> D["a. Translate to noa snapshot<br/>b. Translate blobs/trees to noa objects<br/>c. Store in local redb"]
    D --> E["4. Create merge snapshot (local head + remote head)"]
    E --> F["5. Update workspace head"]
```

## Бэкенд MinIO/S3

Для развёртываний без инфраструктуры Git:

```mermaid
flowchart TD
    A["noa push --remote s3-remote"] --> B["PUT /bucket/snapshots/noa_abc123 (msgpack)"]
    A --> C["PUT /bucket/blobs/&lt;sha256&gt; (raw bytes)"]
    A --> D["PUT /bucket/trees/&lt;sha256&gt; (msgpack)"]
    A --> E["PUT /bucket/refs/default (snapshot ID text)"]
```

Преимущества перед Git-удалённым репозиторием:
- Не требуется трансляция дерева/снимка (нативный формат noa)
- Прямое хранение блобов (без накладных расходов pack-файлов)
- S3-совместимость (работает с AWS, GCS, MinIO, Cloudflare R2)

## Конфигурация удалённых репозиториев

Хранится в `.noa/config` (TOML):

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

## Аутентификация

| Бэкенд | Метод |
|---------|--------|
| Git HTTPS | Учётные данные из `~/.git-credentials` или запрос |
| Git SSH | SSH-агент или файл ключа |
| MinIO/S3 | Ключ доступа + секретный ключ (переменные окружения или конфигурация) |

## Сравнение: Подходы к удалённой совместимости

| Подход | Используется | Плюсы | Минусы |
|----------|---------|------|------|
| Git-мост (gix) | noa | Универсальная совместимость | Накладные расходы трансляции, несоответствие SHA-1/SHA-256 |
| Нативный протокол | Git | Быстро, без трансляции | Работает только с Git |
| WebDAV | SVN | HTTP-стандарт | Ограниченный, специфичный для SVN |
| REST API | Bitbucket | Современный, гибкий | Требует хостингового сервиса |
| S3-совместимое хранилище | noa | Масштабируемое, облачное | Нет совместимости с Git без моста |

noa поддерживает и Git-мост (для совместимости), и нативный S3 (для масштаба).
