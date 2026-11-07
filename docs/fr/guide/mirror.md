# Miroir et reflux

Le système de miroir intègre dans noa le master, tous les tags et les PR heads actifs des dépôts externes (GitHub, etc.) ; la voie de reflux repousse ensuite l'état de noa vers l'origine — formant la transition « noa fait autorité + GitHub en miroir », pendant que farm/CNB continuent de fetch depuis GitHub, sans aucun changement.

## Tâche de miroir (MirrorJob)

```json
{
  "name": "noa",
  "remote_url": "https://github.com/celestia-island/noa.git",
  "github_slug": "celestia-island/noa",
  "branches": ["master"],
  "tags": true,
  "pr_heads": true,
  "backflow": true,
  "canary_push": true,
  "enabled": true
}
```

| Champ | Description |
|---|---|
| `name` | `^[a-z0-9][a-z0-9-]{0,63}$` ; sert aussi de nom de répertoire scratch, de nom de dépôt dans le catalog et de segment de chemin de clone |
| `remote_url` | seuls `https://` / `file://` sont autorisés ; identifiants embarqués interdits (`@`, `?`) |
| `github_slug` | vérification de forme `owner/repo` ; utilisé par la voie Git Data API (pulls tags / têtes de PR) et la sonde canary ; le push de back-flow passe par `remote_url` |
| `branches` | au moins une ; sont refusés `tags`/`tags/*`, `pr/*` (conflit d'espace de noms PR) et les formes illégales d'espaces de travail dérivés |
| `tags` | met en miroir tous les tags |
| `pr_heads` | met en miroir les PR heads actifs (`refs/pull/<N>/head` → `mirror/{job}/pr/<N>`) |
| `backflow` | repousse à chaque vague l'état du scratch vers l'origine (voir plus bas) |
| `canary_push` | pousse une référence sonde pour valider le sens d'écriture |
| `enabled` | une fois désactivée, la tâche est ignorée par le planificateur |

CRUD : `POST/GET /api/v1/mirror/jobs`, `DELETE /api/v1/mirror/jobs/{name}`,
`POST /api/v1/mirror/run` (déclenchement manuel, corps `{"job":"<name>"}`).

## Double voie

- **Voie git** : clone nu scratch + `git fetch` — les tags, les PR heads et les pushes de reflux passent tous par elle.
- **Voie API** : quand le transport git meurt (la famille des secousses réseau), repli automatique vers l'import blob par blob via la Git Data API, avec vérification du SHA de chaque blob (la fidélité ne se dégrade pas selon la voie).

Le repli se joue à l'échelle de la vague : échec de `ensure_scratch` → la vague passe par la voie API → l'auto-guérison supprime le scratch à moitié cassé, et la vague suivante reclone. Le push de reflux ne s'exécute que si la voie git est disponible (l'API ne sait pas pousser de références).

## PR heads

Quand `pr_heads: true`, chaque vague tire en plus `refs/pull/<N>/head` et l'importe sous
`mirror/{job}/pr/<N>` + un espace de travail homonyme. L'espace de noms des PR et celui des branches (branches de forme `pr/<x>`) sont mutuellement exclusifs ; l'upsert le refuse.

## Reflux (backflow)

```text
scratch local refs ──atomic push──▶ origin
  +refs/heads/*:refs/heads/*
  +refs/tags/*:refs/tags/*
  +refs/pull-heads/*:refs/noa/pr/*     (only when pull-heads exist in the scratch)
```

- **Identifiants** : le token résolu depuis `token_env` (`NOA_MIRROR_GH_TOKEN` par défaut) est incorporé sous la forme `x-access-token:<token>` dans une URL de push à usage unique ; le token n'est jamais écrit dans la configuration du scratch.
- **Sémantique de force** : après le basculement, noa est l'autorité d'écriture et les références de l'origine convergent vers le scratch. L'asymétrie des suppressions (une suppression côté noa ne supprime pas la référence côté origine) est une limite connue.
- **Sémantique d'échec** : un échec de reflux place la vague en FAILED — c'est la voie critique de l'acceptation, elle doit rester bien visible.
- **Pont d'espaces de noms** : GitHub refuse les pushes de `refs/pull/*`, aussi les PR heads atterrissent-ils sous
  `refs/noa/pr/<N>`.

## Sonde canary

Quand `canary_push: true`, chaque vague pousse vers l'origine une référence sonde `noa-mirror-canary` (le commit de la première branche répliquée), d'abord via le transport git, avec repli sur la Git Data API en cas d'échec — pour valider en continu les identifiants du sens d'écriture et la connectivité. Un échec de la sonde fait échouer la vague bruyamment.

## Vérification de fidélité

Chaque commit importé fait l'objet d'une réconciliation entrée par entrée : l'ensemble des chemins, le kind des entrées et l'OID des blobs (`git_blob_oid` identique à git) doivent correspondre exactement ; tout écart fait échouer l'entrée et alimente le détail d'erreurs de la vague. Le reflux repose sur cet invariant : pour le contenu importé depuis git, les objets git du scratch et les objets noa correspondent un à un au niveau des blobs — aucun push n'a besoin de couche de conversion. **Limite** : les commits natifs de noa (instantanés purs d'espace de travail) n'ont pas de forme d'objet git et ne relèvent pas du périmètre du reflux (voir la frontière exacte dans [Architecture](./architecture.md)).

## Nettoyage des références périmées (reconcile)

L'ensemble amont des tags et des PR heads constitue l'ensemble de reconcile : les références qui n'apparaissent plus en amont sont purgées de noa (espaces de noms `mirror/{job}/tags/`, `mirror/{job}/pr/`). Le push des tags du reflux se réfère lui aussi à l'amont (une suppression côté noa ne supprime pas côté origine — asymétrie connue).

## Planification

Quand `NOA_MIRROR_ENABLED=1`, le planificateur exécute toutes les tâches enabled toutes les 900 secondes ; `POST /mirror/run` permet un déclenchement manuel. Le détail des vagues (voie, résultat de chaque tâche, état backflow/canary) se lit dans `last_detail` / `last_lane` de
`GET /api/v1/mirror/jobs`.
