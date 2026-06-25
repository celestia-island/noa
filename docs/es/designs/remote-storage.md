# Diseño de Backends de Almacenamiento Remoto

## Visión general

noa admite backends de almacenamiento remoto conectables para distribuir y
respaldar objetos direccionados por contenido. Todos los backends implementan el
mismo trait `ObjectStore`, por lo que las instantáneas, árboles y blobs pueden
enviarse a cualquier backend configurado de forma intercambiable.

## Backends admitidos

| Backend | Identificador de tipo | Transporte | Modelo de distribución |
|---------|----------------------|------------|------------------------|
| Redb (local) | — (siempre local) | KV embebido | Ninguno |
| IPFS (Kubo) | `ipfs` | API HTTP | Par a par (DHT, Bitswap) |
| S3 / MinIO | `s3` | API compatible con S3 | Almacén de objetos centralizado |

## Configuración

Los backends remotos se almacenan como un arreglo `[[storage]]` en `.noa/config`:

```toml
[[storage]]
name = "ipfs-local"
type = "ipfs"
endpoint = "http://127.0.0.1:5001"
gateway = "https://ipfs.io"
auto_pin = false

[[storage]]
name = "s3-backup"
type = "s3"
endpoint = "https://s3.example.com"
bucket = "noa-objects"
access_key = "AKIA..."
secret_key = "..."
region = "us-east-1"
```

Cada entrada tiene un `name` (para referencia CLI), un discriminador `type` y
campos específicos del backend. Los campos desconocidos para un tipo dado se
ignoran.

## Patrón de fábrica

`create_remote_store(&StorageConfig) -> Box<dyn ObjectStore>` inspecciona el
campo `backend_type` y construye la implementación apropiada:

```
type = "ipfs"  →  IpfsObjectStore  (cliente HTTP reqwest → demonio Kubo)
type = "s3"    →  MinioObjectStore (aws-sdk-s3 → punto de conexión compatible con S3)
```

## Puente CID de IPFS

noa identifica los objetos mediante hashes SHA-256 codificados en hexadecimal
(`BlobId`, `TreeId`). Para IPFS, estos se convierten a CIDv1 (códec raw) para
llamadas a la API:

```
bytes CIDv1 = [0x01]           // versión 1
              [0x55]           // códec raw
              [0x12]           // función hash sha2-256
              [0x20]           // longitud de resumen 32 bytes
              [32 bytes hash]

cadena CIDv1 = "b" + base32_lowercase_nopad(bytes CIDv1)
```

Esta conversión es una función pura — el mismo contenido siempre se asigna al
mismo CID. No se requiere un recorrido del demonio para el mapeo.

## Elección de biblioteca: reqwest sobre ipfs-api-backend-hyper

El backend IPFS usa `reqwest` directamente contra la API HTTP de Kubo en lugar
del crate `ipfs-api-backend-hyper`. Justificación:

- `aws-sdk-s3` (ya una dependencia) usa hyper internamente; añadir
  `ipfs-api-backend-hyper` arriesga conflictos de versión de hyper
- La API de Kubo es lo suficientemente simple para llamadas REST ligeras
- `reqwest` con `rustls-tls` evita la dependencia del sistema OpenSSL

## Estrategia de push

Al enviar instantáneas, noa recorre el árbol recursivamente:

1. Para cada blob en el árbol: verificar si existe remotamente → si no, enviarlo
2. Para cada subárbol: recursión
3. Enviar el árbol raíz
4. Para IPFS con `--pin`: fijar el CID raíz para evitar la recolección de basura

Esto asegura que el grafo completo de la instantánea se transfiera. El
`RedbObjectStore` local siempre es la fuente de verdad; los backends remotos son
objetivos de distribución/respaldo.

## Manejo de errores

Los errores específicos del backend se asignan a variantes `NoaError`:

- `IpfsDaemonUnreachable { endpoint }` — conexión rechazada, tiempo de espera
  agotado
- `IpfsError { message }` — respuesta de error de API
- `InvalidCid { cid }` — fallo de conversión SHA-256 → CID
- `ObjectNotFound { id }` — bloque no encontrado en la red/almacén

## Decisiones de diseño

### ¿Por qué una estructura de configuración plana en lugar de enums etiquetados?

TOML no tiene soporte nativo de enums. Una estructura plana con un campo
discriminador `type` más campos opcionales específicos del backend es el enfoque
más amigable con TOML, y coincide con el patrón `RemoteConfig` existente (`name`
+ `url` + `protocol`).

### ¿Por qué no fusionar con `[[remotes]]`?

Los remotos de Git (`RemoteConfig`) y el almacenamiento de objetos
(`StorageConfig`) sirven propósitos diferentes:
- **Remotos** son para push/pull del protocolo git (distribución de código fuente)
- **Almacenamiento** es para distribución de objetos direccionados por contenido
  (instantáneas, blobs)

Mantenerlos separados evita confusión y permite configuración independiente.
