# Guía de Almacenamiento Remoto

## Visión general

noa admite múltiples backends de almacenamiento remoto para distribuir y
respaldar objetos direccionados por contenido. Los backends se configuran por
repositorio y se gestionan mediante el comando unificado `noa storage`.

## Backends admitidos

| Backend | Tipo | Requiere | Caso de uso |
|---------|------|----------|-------------|
| IPFS (Kubo) | `ipfs` | Demonio IPFS en ejecución | Distribución P2P descentralizada |
| S3 / MinIO | `s3` | Punto de conexión compatible con S3 | Respaldo centralizado, almacenamiento en la nube |

## Añadir un backend de almacenamiento

### IPFS

Primero, inicie un demonio Kubo:

```bash
ipfs daemon &   # escucha en 127.0.0.1:5001
```

Añada el backend:

```bash
# Añadir IPFS con valores predeterminados (endpoint=http://127.0.0.1:5001, gateway=https://ipfs.io)
noa storage add ipfs-local --type ipfs

# Personalizar punto de conexión y pasarela
noa storage add ipfs-local --type ipfs \
  --endpoint http://192.168.1.100:5001 \
  --gateway https://dweb.link

# Usar un servicio de fijación remoto (por ejemplo, Pinata)
noa storage add pinata --type ipfs \
  --endpoint https://api.pinata.cloud/psa \
  --auth-token YOUR_PINATA_TOKEN

# Habilitar fijación automática en cada push
noa storage add ipfs-local --type ipfs --auto-pin
```

### S3 / MinIO

```bash
# Añadir un backend compatible con S3
noa storage add s3-backup --type s3 \
  --endpoint https://s3.us-east-1.amazonaws.com \
  --bucket my-noa-objects \
  --access-key AKIAIOSFODNN7EXAMPLE \
  --secret-key wJalrXUtnFEMI/K7MDENG/bPxRfiCYEXAMPLEKEY \
  --region us-east-1

# Añadir un servidor MinIO local
noa storage add minio-local --type s3 \
  --endpoint http://localhost:9000 \
  --bucket noa \
  --access-key minioadmin \
  --secret-key minioadmin
```

## Gestión de backends

```bash
# Listar todos los backends configurados
noa storage list
# ipfs-local    http://127.0.0.1:5001 (ipfs) [gateway=https://ipfs.io]
# s3-backup     https://s3.example.com (s3) [bucket=noa-objects]

# Verificar estado de conexión
noa storage status               # todos los backends
noa storage status ipfs-local    # backend específico

# Eliminar un backend
noa storage remove s3-backup
```

## Envío de instantáneas

Envíe objetos a un backend remoto para distribución o respaldo:

```bash
# Enviar todas las instantáneas a un backend específico
noa storage push --target ipfs-local

# Enviar y fijar (solo IPFS — evita la recolección de basura)
noa storage push --target ipfs-local --pin

# Enviar una instantánea específica
noa storage push --target s3-backup --snapshot noa_abc123

# Enviar todas las instantáneas de un espacio de trabajo
noa storage push --target ipfs-local --workspace feature-auth --pin
```

Con `auto_pin = true` en la configuración, `--pin` está implícito. También puede
enviar a todos los backends de fijación automática a la vez omitiendo `--target`:

```bash
noa storage push --pin   # envía a todos los backends con auto_pin=true
```

## Obtención de objetos

Descargue un objeto de un backend remoto y almacénelo localmente:

```bash
# Obtener por hash SHA-256 (cualquier backend)
noa storage fetch ipfs-local 2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824

# Obtener por CID (solo IPFS)
noa storage fetch ipfs-local bafkreihdwdcefgh4dqkjv67uzcmw7ojel6viejvs6buyq7omaygs5sep7m
```

## Cómo funciona el push

1. **Local primero**: noa lee objetos del `RedbObjectStore` local
2. **Transferencia recursiva**: Para cada instantánea, se recorre todo el árbol
   (blobs y subárboles). Los objetos no presentes en el remoto se transfieren.
3. **Direccionamiento por contenido**: Ambos backends usan SHA-256. Para IPFS,
   los hashes se convierten a CIDv1 (códec raw). Para S3, los hashes se usan como
   claves de objeto.
4. **Fijación** (solo IPFS): Después del push, `--pin` le dice al demonio que
   conserve los objetos, evitando la recolección de basura.

## Formato de configuración

```toml
# .noa/config

[[storage]]
name = "ipfs-local"
type = "ipfs"
endpoint = "http://127.0.0.1:5001"
gateway = "https://ipfs.io"
auto_pin = true

[[storage]]
name = "s3-backup"
type = "s3"
endpoint = "https://s3.example.com"
bucket = "noa-objects"
access_key = "AKIA..."
secret_key = "..."
region = "us-east-1"
```

## Uso programático

```rust
use libnoa::config::StorageConfig;
use libnoa::object::{create_remote_store, ObjectStore};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cfg = StorageConfig::ipfs("local", "http://127.0.0.1:5001");
    let store = create_remote_store(&cfg).await?;

    // Almacenar contenido remotamente
    let blob_id = store.put_blob(b"hello world").await?;
    println!("Stored as: {}", blob_id);

    // Verificar existencia
    assert!(store.has_blob(&blob_id).await?);

    // Recuperar
    let data = store.get_blob(&blob_id).await?;
    assert_eq!(data, b"hello world");

    Ok(())
}
```
