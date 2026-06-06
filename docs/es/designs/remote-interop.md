# Diseño de Interoperabilidad Remota

## Descripción General

noa soporta múltiples backends remotos para sincronizar instantáneas y objetos
entre máquinas y equipos. El objetivo principal de interoperabilidad es Git,
permitiendo una integración perfecta con flujos de trabajo existentes en GitHub, GitLab,
y Bitbucket.

## Trait de Backend Remoto

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

## Capa de Traducción Git

El `GitTranslator` convierte entre el modelo de objetos de noa y el de Git:

### Blob ↔ Git Blob

```mermaid
graph LR
    subgraph Noa
        NB["blob noa:<br/>bytes crudos<br/>hash SHA-256"]
    end
    subgraph Git
        GB["Git blob:<br/>'blob &lt;size&gt;\\0&lt;content&gt;'<br/>hash SHA-1"]
    end
    NB -- "recalcular hash del contenido con<br/>formato de cabecera blob de Git" --> GB
```

### Árbol ↔ Git Tree

```mermaid
graph LR
    subgraph Noa
        NT["árbol noa:<br/>MessagePack [{name, kind, hash}]"]
    end
    subgraph Git
        GT["Git tree:<br/>entradas '&lt;mode&gt; &lt;name&gt;\\0&lt;20-byte-sha1&gt;'"]
    end
    NT -- "TreeEntry::Blob → modo 100644<br/>TreeEntry::Tree → modo 040000<br/>SHA-256 → recalcular SHA-1" --> GT
```

### Instantánea ↔ Git Commit

```mermaid
graph LR
    subgraph "Instantánea noa"
        NS["id: noa_abc123<br/>tree_hash: SHA-256<br/>padres: [noa_...]<br/>autor: agent-001<br/>timestamp: 1717592400000000 (µs)<br/>mensaje: 'añadir funcionalidad'"]
    end
    subgraph "Git Commit"
        GC["tree: SHA-1<br/>parent: SHA-1<br/>author: agent-001 &lt;agent@noa&gt;<br/>message: 'añadir funcionalidad'"]
    end
    NS -- "tree_hash recalculado (SHA-256 → SHA-1)<br/>padres mapeados mediante búsqueda de ID<br/>autor formateado con email ficticio<br/>timestamp µs truncado a segundos" --> GC
```

### Espacio de Trabajo ↔ Git Branch

```mermaid
graph LR
    subgraph Noa
        NW["espacio de trabajo 'feature-1'<br/>(head: noa_abc123)"]
        ND["espacio de trabajo 'default'<br/>(head: noa_def456)"]
    end
    subgraph Git
        GB1["rama 'feature-1'<br/>(HEAD: git-sha1)"]
        GB2["rama 'main'<br/>(HEAD: git-sha1)"]
    end
    NW --> GB1
    ND --> GB2
```

### Mapeo de Referencias

```mermaid
graph LR
    subgraph "Referencias noa"
        NH["HEAD → default"]
        ND2["default → noa_abc"]
        NF["feature-1 → noa_def"]
    end
    subgraph "Referencias Git"
        GH["HEAD → refs/heads/main"]
        GMAIN["refs/heads/main → git-sha1"]
        GF1["refs/heads/feature-1 → git-sha2"]
    end
    NH -.-> GH
    ND2 -.-> GMAIN
    NF -.-> GF1
```

## Flujo de Push

```mermaid
flowchart TD
    A["1. noa push --remote origin"] --> B["2. Cargar todas las instantáneas alcanzables desde las cabezas de los espacios de trabajo"]
    B --> C["3. Traducir cada instantánea → commit Git"]
    C --> D["4. Traducir cada blob/árbol → objeto Git"]
    D --> E["5. Hacer push mediante gix (gitoxide) a la URL de origin"]
    E --> F["6. Actualizar referencias remotas"]
```

## Flujo de Pull

```mermaid
flowchart TD
    A["1. noa pull --remote origin"] --> B["2. Obtener referencias mediante gix"]
    B --> C["3. Para cada nuevo commit Git:"]
    C --> D["a. Traducir a instantánea noa<br/>b. Traducir blobs/árboles a objetos noa<br/>c. Almacenar en redb local"]
    D --> E["4. Crear instantánea de fusión (cabeza local + cabeza remota)"]
    E --> F["5. Actualizar cabeza del espacio de trabajo"]
```

## Backend MinIO/S3

Para despliegues sin infraestructura Git:

```mermaid
flowchart TD
    A["noa push --remote s3-remote"] --> B["PUT /bucket/snapshots/noa_abc123 (msgpack)"]
    A --> C["PUT /bucket/blobs/&lt;sha256&gt; (bytes crudos)"]
    A --> D["PUT /bucket/trees/&lt;sha256&gt; (msgpack)"]
    A --> E["PUT /bucket/refs/default (texto del ID de instantánea)"]
```

Ventajas sobre Git remoto:
- Sin necesidad de traducción de árbol/Instantánea (formato nativo de noa)
- Almacenamiento directo de blobs (sin sobrecarga de archivos pack)
- Compatible con S3 (funciona con AWS, GCS, MinIO, Cloudflare R2)

## Configuración Remota

Almacenado en `.noa/config` (TOML):

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

## Autenticación

| Backend | Método |
|---------|--------|
| Git HTTPS | Credenciales de `~/.git-credentials` o solicitud |
| Git SSH | Agente SSH o archivo de clave |
| MinIO/S3 | Clave de acceso + clave secreta (variables de entorno o configuración) |

## Comparación: Enfoques de Interoperabilidad Remota

| Enfoque | Usado por | Ventajas | Desventajas |
|----------|---------|------|------|
| Puente Git (gix) | noa | Compatibilidad universal | Sobrecarga de traducción, desajuste SHA-1/SHA-256 |
| Protocolo nativo | Git | Rápido, sin traducción | Solo funciona con Git |
| WebDAV | SVN | Estándar HTTP | Limitado, específico de SVN |
| API REST | Bitbucket | Moderno, flexible | Requiere servicio alojado |
| Almacenamiento compatible con S3 | noa | Escalable, nativo de la nube | Sin interoperabilidad Git sin puente |

noa soporta tanto el puente Git (para compatibilidad) como S3 nativo (para escalado).
