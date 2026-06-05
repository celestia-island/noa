# Choix du backend Git : gix (gitoxide) vs git2 (libgit2)

## Statut : Analyse

**Actuel** : `git2 = "0.19"` (binding C vers libgit2)
**Proposé** : `gix = "0.84"` (implémentation Git purement Rust)

## Résumé

gix (gitoxide) est une implémentation Git mature en pur Rust avec une couverture
fonctionnelle suffisante pour remplacer git2 dans le pont git de noa. La migration
élimine une dépendance C (libgit2), réduit les frictions de compilation croisée
et fournit des API Rust idiomatiques.

## Matrice de comparaison

| Critère | git2 (libgit2) | gix (gitoxide) |
|-----------|---------------|----------------|
| **Langage** | C (bindings Rust via crate git2) | Pur Rust |
| **Maturité** | 14 ans, éprouvé en production | 5 ans, développement actif (0.84) |
| **Compilation** | ~15s (reconstruction), nécessite CMake + libgit2-dev | ~8s (reconstruction), cargo uniquement |
| **Compilation croisée** | Pénible (nécessite une chaîne croisée C) | Triviale (compilation croisée cargo) |
| **Style d'API** | Style C, blocs unsafe, durées de vie manuelles | Idiomatique Rust, sûr en emprunt, patterns builder |
| **Gestion d'objets** | git2::Blob, Tree, Commit via ODB | gix::objs::BlobRef, TreeRef, CommitRef |
| **Parcours d'arbre** | Itérateur manuel avec .to_object() | breadthfirst/virtual_roots avec délégué |
| **Push/pull distant** | git2::Remote (fetch, push) | gix::remote (connect, fetch, push) |
| **Pack/pack-index** | Intégré | Complet (crate dédiée : gix-pack) |
| **Réfs** | git2::Reference (lecture/écriture) | gix::refs (support complet des transactions) |
| **Configuration** | Limitée (niveau dépôt) | Configuration en couches (système, utilisateur, dépôt) |
| **SHA-1/256** | SHA-1 uniquement | SHA-1 + SHA-256 (expérimental) |
| **Sécurité mémoire** | Risque de bugs C dans libgit2 | Garanties Rust |
| **Auditabilité** | Nécessite d'auditer le code C de libgit2 | Rust uniquement, cargo-audit |
| **Communauté** | Massive (tous les outils VCS majeurs) | Croissante (gitoxide, crates-index-diff, etc.) |

## Besoins du pont Git de noa

Utilisation actuelle dans `src/git/` :

```rust
// import.rs :
//   - Repository::open()           → gix::open()
//   - repo.head().target()         → gix.head().project_id()
//   - repo.find_commit(oid)        → gix.find_object().try_into_commit()
//   - commit.tree()                → gix.find_object(commit.tree()).try_into_tree()
//   - tree.iter()                  → gix::objs::TreeRefIter
//   - entry.to_object(repo)        → gix.find_object(entry.oid())
//   - obj.kind() === Blob          → obj.kind == ObjectKind::Blob
//   - blob.content()               → blob.data

// translate.rs :
//   - Manipulation pure au niveau octet (pas de dépendance git externe)

// export.rs :
//   - Actuellement todo!() — push utiliserait gix::remote::connect()
//   - Génération de fichiers pack via gix-pack (si nécessaire)
```

Les 6 appels API actuels ont tous des équivalents directs dans gix.

## Couverture fonctionnelle de gix pour noa

| Fonctionnalité nécessaire | Support git2 | Support gix | Notes |
|---------------|-------------|-------------|-------|
| Ouvrir un dépôt | ✅ | ✅ | `gix::open()` ou `gix::ThreadSafeRepository::open()` |
| Lire la réf HEAD | ✅ | ✅ | `gix.head_ref()` / `gix.head()` |
| Trouver un commit par OID | ✅ | ✅ | `gix.find_object(id)?.try_into_commit()` |
| Lire l'arbre depuis le commit | ✅ | ✅ | `gix.find_object(commit.tree())?.try_into_tree()` |
| Itérer les entrées de l'arbre | ✅ | ✅ | `tree.iter()` retourne `TreeRefIter` |
| Lire le contenu d'un blob | ✅ | ✅ | `blob.data` sur `BlobRef` |
| Fetch depuis un distant | ✅ | ✅ | `gix::remote::connect()` |
| Push vers un distant | ✅ | ✅ | `gix::remote::connect()` |
| Clone | ✅ | ✅ | `gix::prepare_clone()` |
| Génération de fichier pack | ✅ | ✅ | crate `gix-pack` |
| Support SHA-256 | ❌ | ✅ (expérimental) | Pertinent pour les instantanés SHA-256 |
| Support asynchrone | ❌ | ✅ (opt-in) | Intéressant pour l'intégration tokio |

## Faisabilité

Toutes les opérations git actuelles et planifiées ont des équivalents dans gix. Le
mapping d'API est direct :

```rust
// git2 (actuel)
let repo = git2::Repository::open(path)?;
let head = repo.head()?;
let commit = repo.find_commit(head.target().unwrap())?;
let tree = commit.tree()?;

// gix (proposé)
let repo = gix::open(path)?;
let head = repo.head_ref()?.expect("HEAD non trouvé");
let head_id = head.id().detach();
let commit = repo.find_object(head_id)?.try_into_commit()
    .map_err(|_| NoaError::Remote("pas un commit".into()))?;
let tree = repo.find_object(commit.tree())?.try_into_tree()
    .map_err(|_| NoaError::Remote("pas un arbre".into()))?;
```

## Plan de migration

### Phase 1 : Remplacer import.rs (opérations en lecture seule)
- Remplacer git2::Repository par gix::ThreadSafeRepository
- Réimplémenter le parcours d'arbre
- Exécuter les tests d'import git existants

### Phase 2 : Remplacer translate.rs
- Aucun changement nécessaire (manipulation pure d'octets, pas de dépendance C)

### Phase 3 : Implémenter export.rs via gix
- Utiliser gix::remote pour le push
- Utiliser gix::prepare_clone pour le clone
- Utiliser gix-pack pour la génération de packfile (si nécessaire côté serveur)

### Phase 4 : Supprimer git2 de Cargo.toml
- Supprimer la dépendance système libgit2
- Vérifier la compilation croisée (x86_64 → aarch64, → wasm à l'avenir)

## Évaluation des risques

| Risque | Probabilité | Impact | Atténuation |
|------|-----------|--------|------------|
| Rupture d'API de gix (0.x) | Moyenne | Faible | Épingler la version, s'adapter aux changements d'API |
| Fonctionnalités avancées manquantes | Faible | Moyen | gix a le push/fetch distant depuis 0.50+ |
| Régression de performance | Faible | Faible | gix souvent plus rapide (pas de surcharge FFI C) |
| Risque d'adoption par la communauté | Faible | Faible | gix est la bibliothèque git Rust de facto |
| Bugs d'interopérabilité SHA-256 | Moyen | Faible | Feature-gated, contournement via translate.rs pur |

## Recommandation

**Migrer vers gix.** Les avantages (zéro dépendance C, sécurité purement Rust,
compilation croisée plus facile, support SHA-256) l'emportent sur les risques
(stabilité API 0.x, communauté plus petite). La migration est à faible risque car :

1. L'utilisation actuelle de git2 est minimale (6 appels API dans import.rs)
2. translate.rs ne nécessite aucun changement
3. export.rs n'est pas implémenté (terrain vierge pour gix)
4. gix est la bibliothèque git Rust standard (utilisée par l'index crates.io)

## Dépendances après migration

```diff
- git2 = "0.19"           # binding C libgit2
+ gix = { version = "0.84", features = ["basic", "index", "pack"] }
```

Aucune nouvelle dépendance système. Construction pure avec `cargo build`.
