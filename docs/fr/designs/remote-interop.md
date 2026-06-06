# Conception de l'interopérabilité distante

## Aperçu

noa prend en charge plusieurs backends distants pour synchroniser les instantanés
et les objets entre machines et équipes. La cible d'interopérabilité principale est
Git, permettant une intégration transparente avec les flux de travail existants sur
GitHub, GitLab et Bitbucket.

## Trait de backend distant

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

## Couche de traduction Git

Le `GitTranslator` convertit entre le modèle d'objets de noa et celui de Git :

### Blob ↔ Blob Git

```mermaid
graph LR
    subgraph Noa
        NB["blob noa :<br/>octets bruts<br/>hachage SHA-256"]
    end
    subgraph Git
        GB["blob Git :<br/>'blob &lt;taille&gt;\\0&lt;contenu&gt;'<br/>hachage SHA-1"]
    end
    NB -- "rehacher le contenu avec<br/>le format d'en-tête de blob Git" --> GB
```

### Arbre ↔ Arbre Git

```mermaid
graph LR
    subgraph Noa
        NT["arbre noa :<br/>MessagePack [{name, kind, hash}]"]
    end
    subgraph Git
        GT["arbre Git :<br/>entrées '&lt;mode&gt; &lt;nom&gt;\\0&lt;20-octets-sha1&gt;'"]
    end
    NT -- "TreeEntry::Blob → mode 100644<br/>TreeEntry::Tree → mode 040000<br/>SHA-256 → SHA-1 rehachage" --> GT
```

### Instantané ↔ Commit Git

```mermaid
graph LR
    subgraph "Instantané noa"
        NS["id: noa_abc123<br/>tree_hash: SHA-256<br/>parents: [noa_...]<br/>auteur: agent-001<br/>horodatage: 1717592400000000 (µs)<br/>message: 'ajouter fonctionnalité'"]
    end
    subgraph "Commit Git"
        GC["arbre: SHA-1<br/>parent: SHA-1<br/>auteur: agent-001 &lt;agent@noa&gt;<br/>message: 'ajouter fonctionnalité'"]
    end
    NS -- "tree_hash rehaché (SHA-256 → SHA-1)<br/>parents mappés via recherche d'ID<br/>auteur formaté avec email factice<br/>horodatage µs tronqué en secondes" --> GC
```

### Espace de travail ↔ Branche Git

```mermaid
graph LR
    subgraph Noa
        NW["espace 'feature-1'<br/>(tête: noa_abc123)"]
        ND["espace 'default'<br/>(tête: noa_def456)"]
    end
    subgraph Git
        GB1["branche 'feature-1'<br/>(HEAD: git-sha1)"]
        GB2["branche 'main'<br/>(HEAD: git-sha1)"]
    end
    NW --> GB1
    ND --> GB2
```

### Mappage des réfs

```mermaid
graph LR
    subgraph "Réfs noa"
        NH["HEAD → default"]
        ND2["default → noa_abc"]
        NF["feature-1 → noa_def"]
    end
    subgraph "Réfs Git"
        GH["HEAD → refs/heads/main"]
        GMAIN["refs/heads/main → git-sha1"]
        GF1["refs/heads/feature-1 → git-sha2"]
    end
    NH -.-> GH
    ND2 -.-> GMAIN
    NF -.-> GF1
```

## Flux Push

```mermaid
flowchart TD
    A["1. noa push --remote origin"] --> B["2. Charger tous les instantanés accessibles depuis les têtes d'espaces"]
    B --> C["3. Traduire chaque instantané → commit Git"]
    C --> D["4. Traduire chaque blob/arbre → objet Git"]
    D --> E["5. Push via gix (gitoxide) vers l'URL d'origine"]
    E --> F["6. Mettre à jour les réfs distantes"]
```

## Flux Pull

```mermaid
flowchart TD
    A["1. noa pull --remote origin"] --> B["2. Récupérer les réfs via gix"]
    B --> C["3. Pour chaque nouveau commit Git :"]
    C --> D["a. Traduire en instantané noa<br/>b. Traduire les blobs/arbres en objets noa<br/>c. Stocker dans le redb local"]
    D --> E["4. Créer un instantané de fusion (tête locale + tête distante)"]
    E --> F["5. Mettre à jour la tête de l'espace de travail"]
```

## Backend MinIO/S3

Pour les déploiements sans infrastructure Git :

```mermaid
flowchart TD
    A["noa push --remote s3-remote"] --> B["PUT /bucket/snapshots/noa_abc123 (msgpack)"]
    A --> C["PUT /bucket/blobs/&lt;sha256&gt; (octets bruts)"]
    A --> D["PUT /bucket/trees/&lt;sha256&gt; (msgpack)"]
    A --> E["PUT /bucket/refs/default (texte de l'ID d'instantané)"]
```

Avantages par rapport au distant Git :
- Pas de traduction arbre/Instantané nécessaire (format noa natif)
- Stockage direct des blobs (pas de surcharge de fichier pack)
- Compatible S3 (fonctionne avec AWS, GCS, MinIO, Cloudflare R2)

## Configuration des distants

Stockée dans `.noa/config` (TOML) :

```toml
[[remotes]]
name = "origin"
url = "https://github.com/exemple/repo.git"
backend = "git"

[[remotes]]
name = "s3"
url = "s3://mon-bucket/noa-repo"
backend = "minio"
endpoint = "https://s3.amazonaws.com"
region = "us-east-1"
```

## Authentification

| Backend | Méthode |
|---------|--------|
| Git HTTPS | Identifiants depuis `~/.git-credentials` ou invite |
| Git SSH | Agent SSH ou fichier de clé |
| MinIO/S3 | Clé d'accès + clé secrète (variables d'env ou config) |

## Comparaison : Approches d'interopérabilité distante

| Approche | Utilisée par | Avantages | Inconvénients |
|----------|---------|------|------|
| Pont Git (gix) | noa | Compatibilité universelle | Surcharge de traduction, discordance SHA-1/SHA-256 |
| Protocole natif | Git | Rapide, pas de traduction | Fonctionne uniquement avec Git |
| WebDAV | SVN | Standard HTTP | Limité, spécifique à SVN |
| API REST | Bitbucket | Moderne, flexible | Nécessite un service hébergé |
| Stockage compatible S3 | noa | Évolutif, cloud-native | Pas d'interopérabilité Git sans pont |

noa prend en charge à la fois le pont Git (pour la compatibilité) et le S3 natif
(pour le passage à l'échelle).
