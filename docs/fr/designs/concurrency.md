# Conception de la concurrence

## Énoncé du problème

Les systèmes VCS traditionnels sérialisent les écritures via un verrou unique ou une
file d'attente de fusion. Cela fonctionne pour les flux de travail à échelle humaine
(10-100 commits/jour) mais s'effondre avec des agents IA produisant des milliers de
modifications de fichiers par minute.

```mermaid
graph LR
    subgraph Problème
        A["100 agents IA × 10 écritures/s = 1000 écritures/s"]
    end
    subgraph Traditionnel
        B["Git/SVN : verrou unique → file<br/>~100 écritures/s de débit"]
    end
    subgraph Noa
        C["noa : journaux en ajout seul<br/>~10 000+ écritures/s de débit"]
    end
```

## Architecture

### Couche 1 : AgentLog (Chemin d'écriture)

Chaque espace de travail possède un fichier JSONL dédié sous `.noa/agent-logs/`.

```mermaid
graph LR
    ws1["espace 'agent-001'"] --> f1["agent-logs/agent-001.log"]
    ws2["espace 'agent-002'"] --> f2["agent-logs/agent-002.log"]
```

Les écritures utilisent le drapeau `O_APPEND`, qui fournit :
- **Atomicité** : Le noyau garantit l'atomicité de l'écriture entière pour les ajouts
- **Ordonnancement** : Les écritures sont sérialisées par fichier (par espace de travail)
- **Pas de verrouillage** : Aucun fcntl/flock requis entre différents fichiers

```rust
pub trait AgentLog: Send + Sync {
    async fn append(&self, workspace: &str, entry: &LogEntry) -> Result<()>;
    async fn read_all(&self, workspace: &str) -> Result<Vec<LogEntry>>;
}
```

### Couche 2 : Magasin d'instantanés (Chemin de lecture)

Les instantanés sont stockés dans redb avec MVCC (contrôle de concurrence multi-version) :
- Les écritures sont sérialisées via la transaction d'écrivain unique de redb
- Les lectures ne bloquent jamais les écritures (isolation d'instantané)
- Plusieurs lecteurs peuvent accéder simultanément

### Couche 3 : Consolidation (Chemin de fusion)

Le `Consolidator` lit tous les journaux d'agents à travers les espaces de travail,
trie par horodatage et produit une chaîne d'instantanés unifiée :

```mermaid
graph TD
    subgraph Entrée
        L1["agent-001.log : [write A@t1, write B@t3]"]
        L2["agent-002.log : [write C@t2, write D@t4]"]
    end
    subgraph Consolidé
        C1["write A@t1 → write C@t2 → write B@t3 → write D@t4"]
    end
    L1 --> C1
    L2 --> C1
```

Cela s'exécute de manière asynchrone et ne bloque pas les écritures des agents.

## Garanties de concurrence

| Garantie | Mécanisme |
|-----------|-----------|
| Aucune perte de données | O_APPEND + fsync par écriture |
| Ordonnancement par espace | Fichier unique par espace de travail |
| Ordonnancement inter-espaces | Horodatages en microsecondes |
| Cohérence de lecture | Isolation d'instantané MVCC de redb |
| Sécurité de tête d'espace | Mises à jour CAS (compare-and-swap) |

## Analyse de scalabilité

### Mono-processus (Embarqué)

| Agents | 1-100 (même processus) |
| Débit | ~10 000 écritures/s |
| Goulot | E/S disque (fsync par écriture) |

### Multi-processus (noa-server)

| Agents | 100-1000 (processus séparés) |
| Débit | ~5 000 écritures/s |
| Goulot | Sérialisation des écritures côté serveur |

Le serveur détient une seule connexion à la base de données et sérialise les
écritures. Les journaux d'agents restent par fichier pour l'ingestion parallèle.

### Distribué (Backend MinIO)

| Agents | 1000+ |
| Débit | Limite de débit S3 PUT (~3 500/s par préfixe) |
| Goulot | réseau + limites de débit S3 |

## Comparaison avec les alternatives

### Git + Verrouillage de fichiers

```mermaid
graph LR
    A["Problème : Verrous consultatifs, pas d'application forcée"]
    B["Contention : Élevée (mise à jour d'une seule ref par push)"]
    C["Résolution : Fusion manuelle requise"]
```

### SVN + svn:needs-lock

```mermaid
graph LR
    A["Problème : Les verrous au niveau fichier bloquent tous les autres rédacteurs"]
    B["Contention : Très élevée (commits sérialisés)"]
    C["Résolution : Attente de verrou → timeout → échec"]
```

### Transformation opérationnelle (OT)

```mermaid
graph LR
    A["Problème : Algorithme complexe, difficile à implémenter correctement"]
    B["Contention : Faible (transformation en mémoire)"]
    C["Résolution : Automatique, mais nécessite un serveur centralisé"]
```

### CRDT (Types de données répliqués sans conflit)

```mermaid
graph LR
    A["Problème : Surcharge de métadonnées importante, cohérence éventuelle"]
    B["Contention : Aucune"]
    C["Résolution : Automatique, mais peut produire des résultats inattendus"]
```

### L'approche de noa

```mermaid
graph LR
    A["Problème : Les écritures d'agents sont éphémères et régénérables"]
    B["Approche : Journaux en ajout seul + consolidation asynchrone"]
    C["Contention : Aucune pour les écritures, sérialisée pour les instantanés"]
    D["Résolution : upstream-wins par défaut + réapplication par l'agent"]
```

## Stratégie fsync

Chaque écriture dans le journal d'agent suit ce modèle :

```rust
file.write_all(data)?;   // ajouter au fichier
file.flush()?;           // vider le tampon utilisateur
file.sync_data()?;       // fsync — garantir la durabilité sur disque
```

Sur Linux, `sync_data()` évite la synchronisation des métadonnées (fdatasync),
réduisant la latence d'environ 30 % par rapport à un fsync complet.

## Futur : Regroupement par lots du journal de pré-écriture

Actuel : un fsync par écriture.
Planifié : regrouper plusieurs écritures en un seul fsync :

```rust
// L'agent met en mémoire tampon les écritures
agent.buffer(write_a);
agent.buffer(write_b);
agent.buffer(write_c);
agent.flush(); // un seul fsync pour les trois
```

Amélioration de débit attendue : 3-5x pour les écritures en rafale.
