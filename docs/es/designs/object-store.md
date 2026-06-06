# Diseño de Almacenamiento de Objetos

## Descripción General

noa utiliza un modelo de almacenamiento direccionado por contenido inspirado en Git pero con una
arquitectura de backend intercambiable. Los objetos se direccionan mediante hash SHA-256
y se almacenan como blobs opacos.

## Tipos de Objetos

### Blob

Contenido de archivo sin procesar. Identificado por `SHA256(content)`.

```rust
pub struct BlobId(pub String); // SHA-256 codificado en hexadecimal
```

Sin compresión delta. Cada contenido único produce exactamente un blob.
El contenido duplicado se deduplica automáticamente por hash.

### Árbol

Listado de directorio. Mapea rutas a entradas hijas (blobs o subárboles).

```rust
pub struct TreeEntry {
    pub name: String,
    pub kind: TreeEntryKind, // Blob o Tree
    pub hash: String,        // SHA-256 del hijo
}

pub struct TreeId(pub String); // SHA-256(msgpack(entries))
```

Los árboles se serializan como MessagePack para compacidad y deserialización rápida.

## Definición del Trait

```rust
#[async_trait]
pub trait ObjectStore: Send + Sync {
    async fn put_blob(&self, data: &[u8]) -> Result<BlobId>;
    async fn get_blob(&self, id: &BlobId) -> Result<Vec<u8>>;
    async fn put_tree(&self, entries: Vec<TreeEntry>) -> Result<TreeId>;
    async fn get_tree(&self, id: &TreeId) -> Result<Vec<TreeEntry>>;
}
```

## Backends

### RedbObjectStore (Local)

Utiliza el almacén clave-valor embebido [redb](https://github.com/cberner/redb).

- Dos tablas: `blobs` (clave: bytes del hash, valor: bytes del contenido) y
  `trees` (clave: bytes del hash, valor: entradas msgpack)
- Lecturas sin copia mediante archivos mapeados en memoria
- Transacciones ACID con recuperación automática ante fallos
- Escritor único, múltiples lectores mediante MVCC
- Sin necesidad de demonio externo

### MinioObjectStore (Remoto)

Utiliza API compatible con S3 mediante `aws-sdk-s3`.

- Direccionamiento estilo ruta: `<bucket>/blobs/<hash>`, `<bucket>/trees/<hash>`
- Soporta cualquier backend compatible con S3 (MinIO, AWS S3, GCS, etc.)
- Reintentos automáticos con retroceso exponencial
- Adecuado para despliegues distribuidos

## Decisiones de Diseño

### ¿Por qué SHA-256 en lugar de SHA-1?

Git usa SHA-1, que está criptográficamente roto (ataque SHAttered, 2017).
SHA-256 es resistente a colisiones y está ampliamente disponible.

### ¿Por qué sin compresión delta?

1. **Simplicidad**: La compresión delta (archivos pack de Git) añade una complejidad
   significativa (emparejamiento por ventana deslizante, thin packs, cadenas delta).
2. **Rendimiento de escritura**: Las escrituras directas de blob son O(1). La compresión delta
   requiere leer objetos existentes.
3. **Carga de trabajo de agentes IA**: Los agentes frecuentemente regeneran archivos completos.
   Las versiones antiguas son efímeras — las cadenas delta serían cortas y numerosas.
4. **Descarga al backend**: S3/MinIO manejan la deduplicación en la capa de almacenamiento.

### ¿Por qué MessagePack para árboles?

- 30-50% más pequeño que JSON para datos con mucho contenido binario
- Esquema flexible (sin necesidad de definiciones protobuf)
- Soporte del ecosistema Rust mediante `rmp-serde`
- Deserialización rápida

### ¿Por qué redb en lugar de SQLite?

- **Seguridad de tipos**: redb usa genéricos de Rust para definiciones de tablas
- **Rendimiento**: redb está optimizado para cargas de trabajo Rust (lecturas sin copia)
- **Simplicidad**: Una sola dependencia, sin enlace a bibliotecas C
- **Seguridad ante fallos**: El registro de escritura anticipada de redb es más simple que el modo WAL de SQLite

Compensación: redb tiene una comunidad más pequeña y menos opciones de herramientas que SQLite.
Para el caso de uso de noa (almacenamiento binario embebido), la compensación es favorable.
