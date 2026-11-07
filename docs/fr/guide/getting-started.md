# Prise en main

Cette page vous conduit de zéro jusqu'à un noa en cours d'exécution : construire,
démarrer le serveur et réaliser votre premier clone / push.

## Prérequis

- Rust stable ≥ 1.99 (via `rustup`)
- git ≥ 2.39 (le serveur pilote directement `upload-pack` / `receive-pack` en HTTP
  intelligent)
- Optionnel : node + pnpm (pour construire la WebUI)

## Construction

```bash
git clone https://github.com/celestia-island/noa.git
cd noa
cargo build --release --locked
```

Artefacts : `target/release/noa` (CLI) et `target/release/noa-server` (serveur).

## Démarrer le serveur

La configuration minimale tient en un jeton d'API (l'identifiant admin d'amorçage) :

```bash
export NOA_API_TOKEN="<your-token>"
export NOA_MIRROR_SCRATCH_DIR=/var/lib/noa/mirror-cache   # mirror scratch root
export NOA_MIRROR_ENABLED=1                               # enable mirror scheduling (default 900s)
export NOA_MIRROR_GH_TOKEN="<github-pat>"                 # optional: back-flow / canary push credential
noa-server --host 0.0.0.0 --port 3016
```

Une fois démarré :

```bash
curl -H "Authorization: Bearer <your-token>" http://127.0.0.1:3016/api/v1/repos
```

Un retour `[]` signifie que le serveur est prêt. Chaque endpoint REST exige
l'authentification par jeton porteur (bearer) ; les lectures `/git/{repo}/*` l'exigent
aussi (les requêtes non authentifiées reçoivent une erreur 401 accompagnée d'un défi
`Basic`).

## Le flux de travail natif noa (recommandé)

Le CLI `noa` est le point d'entrée de premier rang — un clone est un dépôt où `.git` et
`.noa` coexistent (git gère les sources, noa gère les données d'agents) :

```bash
noa clone http://<token>@127.0.0.1:3016/git/hello --path hello
cd hello
noa workspace create agent-a     # a per-agent isolated workspace
# … edit files …
noa snapshot create -m "change" -a agent-a
noa log                          # this workspace's snapshot history
noa push                         # publishes the workspace snapshot (serialized to a git commit)
```

`noa push` publie **l'état de l'instantané de l'espace de travail** (il crée un commit
`[noa export]`) — pas les commits de `.git`. Les deux voies sont parallèles ; ne les
mélangez pas (exécutez `noa snapshot create` avant de pousser, sinon vous publiez un
instantané périmé). L'authentification est injectée depuis les variables d'environnement
`NOA_GIT_USER` + `NOA_GIT_PASS`. Le répertoire entier (y compris `.git/noa`) se copie
sur une autre machine et continue de fonctionner.

## La voie de compatibilité git (tout outil git fonctionne)

Le même endpoint `/git/{repo}` parle aussi le HTTP intelligent standard de git — git
brut, CI et farm fonctionnent sans modification :

```bash
git clone http://<token>@127.0.0.1:3016/git/hello
```

Les pushs (receive-pack) exigent le rôle d'écriture ; un push atterri **déclenche
immédiatement la vague d'import** (le retour suit sur la même vague). C'est la forme de
transition qui garde farm/CNB à zéro changement.

## Votre premier miroir (importer un dépôt GitHub dans noa)

```bash
curl -X POST -H "Authorization: Bearer <your-token>" \
  -H "Content-Type: application/json" \
  -d '{
    "name": "hello",
    "remote_url": "https://github.com/you/hello.git",
    "github_slug": "you/hello",
    "branches": ["master"],
    "tags": true,
    "pr_heads": true,
    "enabled": true
  }' \
  http://127.0.0.1:3016/api/v1/mirror/jobs

curl -X POST -H "Authorization: Bearer <your-token>" \
  -H "Content-Type: application/json" \
  -d '{"job":"hello"}' \
  http://127.0.0.1:3016/api/v1/mirror/run
```

Une fois la vague terminée, le dépôt est clonable via noa :

```bash
git clone http://<your-token>@127.0.0.1:3016/git/hello
```

## Sauvegarde et migration

Tout l'état du serveur s'exporte en un répertoire portable (voir
[Déploiement](../reference/deploy.md) § sauvegarde) : un instantané redb cohérent + un
bundle git clonable par dépôt + un manifeste sha256 ; `noa-server --import-state <dir>`
restaure hors ligne. Le critère d'acceptation est l'exercice : export → restauration sur
une autre machine → service → réconciliation.


## Pour aller plus loin

- [Architecture](./architecture.md) — le modèle espace de travail / instantané /
  références
- [Miroir et voie de retour](./mirror.md) — les deux voies, têtes de PR, retour,
  canary
- [Référence de l'API HTTP](../reference/http-api.md) — chaque endpoint
