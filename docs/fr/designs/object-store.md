# Conception du stockage d'objets

## Aperçu

noa utilise un modèle de stockage adressé par contenu inspiré de Git mais avec une
architecture backend enfichable. Les objets sont adressés par un hachage SHA-256
et stockés sous forme de blobs opaques.

## Types d'objets

### Blob

Contenu brut d'un fichier. Identifié par `SHA256(contenu)`.

```rust
pub struct BlobId(pub String); // SHA-256 encodé en hexadécimal
```

Pas de compression delta. Chaque contenu unique produit exactement un blob.
Le contenu dupliqué est automatiquement dédupliqué par le hachage.

### Arbre (Tree)

Liste de répertoire. Associe des chemins à des entrées enfants (blobs ou sous-arbres).

```rust
pub struct TreeEntry {
    pub name: String,
    pub kind: TreeEntryKind, // Blob ou Tree
    pub hash: String,        // SHA-256 de l'enfant
}

pub struct TreeId(pub String); // SHA-256(msgpack(entries))
```

Les arbres sont sérialisés en MessagePack pour la compacité et la désérialisation rapide.

## Définition du trait

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

Utilise le magasin clé-valeur embarqué [redb](https://github.com/cberner/redb).

- Deux tables : `blobs` (clé : octets de hachage, valeur : octets de contenu) et
  `trees` (clé : octets de hachage, valeur : entrées msgpack)
- Lectures zéro-copie via fichiers mappés en mémoire
- Transactions ACID avec récupération automatique après crash
- Écrivain unique, plusieurs lecteurs via MVCC
- Aucun démon externe requis

### MinioObjectStore (Distant)

Utilise une API compatible S3 via `aws-sdk-s3`.

- Adressage par chemin : `<bucket>/blobs/<hash>`, `<bucket>/trees/<hash>`
- Prend en charge tout backend compatible S3 (MinIO, AWS S3, GCS, etc.)
- Tentatives automatiques avec backoff exponentiel
- Adapté aux déploiements distribués

## Décisions de conception

### Pourquoi SHA-256 plutôt que SHA-1 ?

Git utilise SHA-1, qui est cryptographiquement cassé (attaque SHAttered, 2017).
SHA-256 est résistant aux collisions et largement disponible.

### Pourquoi pas de compression delta ?

1. **Simplicité** : La compression delta (fichiers pack de Git) ajoute une complexité
   significative (correspondance par fenêtre glissante, packs minces, chaînes de delta).
2. **Performance d'écriture** : Les écritures directes de blob sont O(1). La compression
   delta nécessite la lecture des objets existants.
3. **Charge de travail des agents IA** : Les agents régénèrent fréquemment des fichiers
   entiers. Les anciennes versions sont éphémères — les chaînes de delta seraient courtes
   et nombreuses.
4. **Délégation au backend** : S3/MinIO gèrent la déduplication au niveau du stockage.

### Pourquoi MessagePack pour les arbres ?

- 30-50% plus petit que JSON pour les données à forte composante binaire
- Schéma flexible (pas besoin de définitions protobuf)
- Support de l'écosystème Rust via `rmp-serde`
- Désérialisation rapide

### Pourquoi redb plutôt que SQLite ?

- **Sécurité de type** : redb utilise les génériques Rust pour les définitions de tables
- **Performance** : redb est optimisé pour les charges de travail Rust (lectures zéro-copie)
- **Simplicité** : Dépendance unique, pas de liaison avec une bibliothèque C
- **Sécurité anti-crash** : Le journal de pré-écriture de redb est plus simple que le mode WAL de SQLite

Compromis : redb a une communauté plus petite et moins d'outils que SQLite.
Pour le cas d'usage de noa (stockage binaire embarqué), le compromis est favorable.
