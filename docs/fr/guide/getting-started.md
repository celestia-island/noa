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

## Votre premier push (autorité d'écriture)

Les pushs passent par le chemin receive-pack de `/git/{repo}` et exigent une identité
authentifiée :

```bash
cd hello
echo "hello noa" > README.md
git add -A && git commit -m "hello noa"
git push origin master
```

Le push atterrit dans le scratch du miroir ; la vague de miroir suivante l'importe
dans le système d'instantanés/références de noa et, si la tâche a `backflow` activé,
l'état est également renvoyé vers l'origine (GitHub) — la forme de transition
« autorité noa + miroir GitHub ».

## Pour aller plus loin

- [Architecture](./architecture.md) — le modèle espace de travail / instantané /
  références
- [Miroir et voie de retour](./mirror.md) — les deux voies, têtes de PR, retour,
  canary
- [Référence de l'API HTTP](../reference/http-api.md) — chaque endpoint
