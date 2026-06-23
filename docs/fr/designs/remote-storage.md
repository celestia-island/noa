# Conception des Backends de Stockage Distant

## Aperçu

noa prend en charge des backends de stockage distant enfichables pour la
distribution et la sauvegarde d'objets à adressage de contenu. Tous les backends
implémentent le même trait `ObjectStore`, de sorte que les instantanés, arbres et
blobs peuvent être envoyés vers n'importe quel backend configuré de manière
interchangeable.

## Backends pris en charge

| Backend | Identifiant de type | Transport | Modèle de distribution |
|---------|---------------------|-----------|------------------------|
| Redb (local) | — (toujours local) | KV embarqué | Aucun |
| IPFS (Kubo) | `ipfs` | API HTTP | Pair-à-pair (DHT, Bitswap) |
| S3 / MinIO | `s3` | API compatible S3 | Magasin d'objets centralisé |

## Configuration

Les backends distants sont stockés sous forme de tableau `[[storage]]` dans
`.noa/config` :

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

Chaque entrée possède un `name` (pour la référence CLI), un discriminateur
`type` et des champs spécifiques au backend. Les champs inconnus pour un type
donné sont ignorés.

## Modèle de fabrique

`create_remote_store(&StorageConfig) -> Box<dyn ObjectStore>` inspecte le champ
`backend_type` et construit l'implémentation appropriée :

```
type = "ipfs"  →  IpfsObjectStore  (client HTTP reqwest → démon Kubo)
type = "s3"    →  MinioObjectStore (aws-sdk-s3 → point de terminaison compatible S3)
```

## Pont CID IPFS

noa identifie les objets par des hachages SHA-256 encodés en hexadécimal
(`BlobId`, `TreeId`). Pour IPFS, ceux-ci sont convertis en CIDv1 (codec raw) pour
les appels d'API :

```
CIDv1 octets = [0x01]           // version 1
               [0x55]           // codec raw
               [0x12]           // fonction de hachage sha2-256
               [0x20]           // longueur de condensat 32 octets
               [32 octets de hachage]

Chaîne CIDv1 = "b" + base32_lowercase_nopad(octets CIDv1)
```

Cette conversion est une fonction pure — le même contenu correspond toujours au
même CID. Aucun aller-retour vers le démon n'est requis pour le mappage.

## Choix de bibliothèque : reqwest plutôt que ipfs-api-backend-hyper

Le backend IPFS utilise `reqwest` directement contre l'API HTTP Kubo plutôt que
le crate `ipfs-api-backend-hyper`. Justification :

- `aws-sdk-s3` (déjà une dépendance) utilise hyper en interne ; ajouter
  `ipfs-api-backend-hyper` risque de provoquer des conflits de version hyper
- L'API Kubo est suffisamment simple pour des appels REST légers
- `reqwest` avec `rustls-tls` évite la dépendance système OpenSSL

## Stratégie de push

Lors du push d'instantanés, noa parcourt l'arbre récursivement :

1. Pour chaque blob dans l'arbre : vérifier s'il existe à distance → sinon, le
   pousser
2. Pour chaque sous-arbre : récursivité
3. Pousser l'arbre racine
4. Pour IPFS avec `--pin` : épingler le CID racine pour éviter le nettoyage par
   ramasse-miettes

Cela garantit que le graphe complet de l'instantané est transféré. Le
`RedbObjectStore` local est toujours la source de vérité ; les backends distants
sont des cibles de distribution/sauvegarde.

## Gestion des erreurs

Les erreurs spécifiques au backend sont mappées vers des variantes `NoaError` :

- `IpfsDaemonUnreachable { endpoint }` — connexion refusée, délai d'attente
  dépassé
- `IpfsError { message }` — réponse d'erreur d'API
- `InvalidCid { cid }` — échec de conversion SHA-256 → CID
- `ObjectNotFound { id }` — bloc introuvable sur le réseau/le magasin

## Décisions de conception

### Pourquoi une structure de configuration plate plutôt que des énumérations étiquetées ?

TOML n'a pas de support natif des énumérations. Une structure plate avec un champ
discriminateur `type` plus des champs optionnels spécifiques au backend est
l'approche la plus adaptée à TOML, et correspond au modèle `RemoteConfig`
existant (`name` + `url` + `protocol`).

### Pourquoi ne pas fusionner avec `[[remotes]]` ?

Les dépôts distants Git (`RemoteConfig`) et le stockage d'objets
(`StorageConfig`) servent des objectifs différents :
- **Les dépôts distants** sont pour le push/pull du protocole git (distribution de
  code source)
- **Le stockage** est pour la distribution d'objets à adressage de contenu
  (instantanés, blobs)

Les garder séparés évite la confusion et permet une configuration indépendante.
