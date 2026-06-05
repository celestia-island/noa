# Démarrage

## Prérequis

- Rust 1.75+ (stable)
- Python 3.8+ (pour les scripts de build)
- Exécuteur de commandes `just`

## Installation

```bash
git clone https://github.com/celestia-island/noa.git
cd noa
just init          # récupérer les dépendances
just build-dev     # build de développement
```

Le binaire `noa` se trouve dans `target/debug/noa`.

## Démarrage rapide

```bash
# Initialiser un nouveau dépôt
noa init .

# Vérifier le statut
noa status
# Sur l'espace : default

# Créer un espace de travail
noa workspace create feature-1

# Changer vers celui-ci
noa workspace switch feature-1

# Créer un instantané
noa snapshot create -m "travail initial"

# Voir l'historique
noa log

# Revenir et fusionner
noa workspace switch default
noa workspace merge feature-1

# Gérer les dépôts distants
noa remote add origin https://github.com/exemple/repo.git
noa remote list
```

## Exécution des exemples

```bash
python3 examples/run_all.py
```

## Développement

```bash
just fmt            # formater le code
just clippy         # lint
just test           # exécuter les tests
just check          # vérification de type
```
