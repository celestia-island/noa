# Guide de Stockage Distant

## Aperçu

noa prend en charge plusieurs backends de stockage distant pour la distribution
et la sauvegarde d'objets à adressage de contenu. Les backends sont configurés
par dépôt et gérés via la commande unifiée `noa storage`.

## Backends pris en charge

| Backend | Type | Requiert | Cas d'usage |
|---------|------|----------|-------------|
| IPFS (Kubo) | `ipfs` | Démon IPFS en cours d'exécution | Distribution P2P décentralisée |
| S3 / MinIO | `s3` | Point de terminaison compatible S3 | Sauvegarde centralisée, stockage cloud |

## Ajout d'un backend de stockage

### IPFS

Tout d'abord, démarrez un démon Kubo :

```bash
ipfs daemon &   # écoute sur 127.0.0.1:5001
```

Ajoutez le backend :

```bash
# Ajouter IPFS avec les valeurs par défaut (endpoint=http://127.0.0.1:5001, gateway=https://ipfs.io)
noa storage add ipfs-local --type ipfs

# Personnaliser le point de terminaison et la passerelle
noa storage add ipfs-local --type ipfs \
  --endpoint http://192.168.1.100:5001 \
  --gateway https://dweb.link

# Utiliser un service d'épinglage distant (par exemple, Pinata)
noa storage add pinata --type ipfs \
  --endpoint https://api.pinata.cloud/psa \
  --auth-token YOUR_PINATA_TOKEN

# Activer l'épinglage automatique à chaque push
noa storage add ipfs-local --type ipfs --auto-pin
```

### S3 / MinIO

```bash
# Ajouter un backend compatible S3
noa storage add s3-backup --type s3 \
  --endpoint https://s3.us-east-1.amazonaws.com \
  --bucket my-noa-objects \
  --access-key AKIAIOSFODNN7EXAMPLE \
  --secret-key wJalrXUtnFEMI/K7MDENG/bPxRfiCYEXAMPLEKEY \
  --region us-east-1

# Ajouter un serveur MinIO local
noa storage add minio-local --type s3 \
  --endpoint http://localhost:9000 \
  --bucket noa \
  --access-key minioadmin \
  --secret-key minioadmin
```

## Gestion des backends

```bash
# Lister tous les backends configurés
noa storage list
# ipfs-local    http://127.0.0.1:5001 (ipfs) [gateway=https://ipfs.io]
# s3-backup     https://s3.example.com (s3) [bucket=noa-objects]

# Vérifier l'état de la connexion
noa storage status               # tous les backends
noa storage status ipfs-local    # backend spécifique

# Supprimer un backend
noa storage remove s3-backup
```

## Pousser des instantanés

Poussez des objets vers un backend distant pour la distribution ou la
sauvegarde :

```bash
# Pousser tous les instantanés vers un backend spécifique
noa storage push --target ipfs-local

# Pousser et épingler (IPFS uniquement — évite le nettoyage par ramasse-miettes)
noa storage push --target ipfs-local --pin

# Pousser un instantané spécifique
noa storage push --target s3-backup --snapshot noa_abc123

# Pousser tous les instantanés d'un espace de travail
noa storage push --target ipfs-local --workspace feature-auth --pin
```

Avec `auto_pin = true` dans la configuration, `--pin` est implicite. Vous pouvez
également pousser vers tous les backends à épinglage automatique à la fois en
omettant `--target` :

```bash
noa storage push --pin   # pousse vers tous les backends avec auto_pin=true
```

## Récupération d'objets

Téléchargez un objet depuis un backend distant et stockez-le localement :

```bash
# Récupérer par hachage SHA-256 (n'importe quel backend)
noa storage fetch ipfs-local 2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824

# Récupérer par CID (IPFS uniquement)
noa storage fetch ipfs-local bafkreihdwdcefgh4dqkjv67uzcmw7ojel6viejvs6buyq7omaygs5sep7m
```

## Comment fonctionne le push

1. **Local d'abord** : noa lit les objets depuis le `RedbObjectStore` local
2. **Transfert récursif** : Pour chaque instantané, l'arbre entier (blobs et
   sous-arbres) est parcouru. Les objets non présents sur le distant sont
   transférés.
3. **Adressage de contenu** : Les deux backends utilisent SHA-256. Pour IPFS, les
   hachages sont convertis en CIDv1 (codec raw). Pour S3, les hachages sont
   utilisés comme clés d'objet.
4. **Épinglage** (IPFS uniquement) : Après le push, `--pin` indique au démon de
   conserver les objets, évitant le nettoyage par ramasse-miettes.

## Format de configuration

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

## Utilisation programmatique

```rust
use libnoa::config::StorageConfig;
use libnoa::object::{create_remote_store, ObjectStore};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cfg = StorageConfig::ipfs("local", "http://127.0.0.1:5001");
    let store = create_remote_store(&cfg).await?;

    // Stocker du contenu à distance
    let blob_id = store.put_blob(b"hello world").await?;
    println!("Stored as: {}", blob_id);

    // Vérifier l'existence
    assert!(store.has_blob(&blob_id).await?);

    // Récupérer
    let data = store.get_blob(&blob_id).await?;
    assert_eq!(data, b"hello world");

    Ok(())
}
```
