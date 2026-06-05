# Référence CLI

## `noa init [chemin]`

Initialiser un nouveau dépôt `.noa/`. Crée `noa.redb`, `agent-logs/`, `HEAD` et `config`.

```bash
noa init .           # répertoire courant
noa init /chemin/repo  # chemin spécifique
```

## `noa status`

Afficher l'espace de travail courant et l'instantané de tête.

```bash
noa status
# Sur l'espace : default (tête : noa_abc123, msg : initial)
```

## `noa log [options]`

Afficher l'historique des instantanés.

| Drapeau | Défaut | Description |
|------|---------|-------------|
| `-w, --workspace` | HEAD courant | Filtrer par espace de travail |
| `-l, --limit` | 20 | Nombre maximal d'entrées à afficher |

```bash
noa log
noa log --workspace feature-1 --limit 50
```

## `noa snapshot <sous-commande>`

### `noa snapshot create [-m msg] [-a auteur]`

Créer un instantané depuis le journal d'agent de l'espace de travail courant.

```bash
noa snapshot create -m "ajouter la fonctionnalité login" -a "agent-001"
```

### `noa snapshot list`

Lister tous les instantanés à travers les espaces de travail.

### `noa snapshot diff <a> <b>`

Afficher les différences au niveau fichier entre deux instantanés.

```bash
noa snapshot diff noa_abc123 noa_def456
```

## `noa workspace <sous-commande>`

### `noa workspace create <nom> [--agent <id>]`

Créer un nouvel espace de travail fork depuis le HEAD courant.

### `noa workspace switch <nom>`

Changer l'espace de travail actif (met à jour HEAD).

### `noa workspace list`

Lister tous les espaces de travail. `*` marque l'espace actif.

### `noa workspace delete <nom>`

Supprimer un espace de travail (impossible de supprimer l'espace actif).

### `noa workspace merge <depuis>`

Fusionner un autre espace de travail dans le courant en utilisant la fusion à trois voies.

```bash
noa workspace switch default
noa workspace merge feature-1
```

## `noa remote <sous-commande>`

### `noa remote add <nom> <url>`

Ajouter un dépôt distant.

### `noa remote remove <nom>`

Supprimer un dépôt distant.

### `noa remote list`

Lister tous les dépôts distants configurés.

## `noa push [--remote nom]`

Pousser vers un dépôt distant (pas encore implémenté).

## `noa pull [--remote nom]`

Tirer depuis un dépôt distant (pas encore implémenté).

## `noa fetch [--remote nom]`

Récupérer depuis un dépôt distant sans fusionner (pas encore implémenté).

## `noa clone <url> [chemin]`

Cloner un dépôt distant (pas encore implémenté).
