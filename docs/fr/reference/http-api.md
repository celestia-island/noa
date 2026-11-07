# Référence de l'API HTTP

Base : `http://<host>:<port>/api/v1`. Tous les endpoints exigent une authentification bearer (`Authorization: Bearer
<token>`) ; non authentifié = 401 + challenge Basic. Application des rôles : écriture par reader = 403 ; `/users` hors
GET réservé à l'admin.

La forme d'erreur est uniformisée en `{"error": "..."}` ; pagination en ligne (`?limit=` 1..=1000, `?offset=`).

## repos — catalogue des dépôts

| Méthode | Chemin | Description |
|---|---|---|
| GET | `/repos` | liste le catalog |
| POST | `/repos` | enregistre (`name` obligatoire, même forme que le nom d'un job de miroir) |
| GET | `/repos/{name}` | un seul |
| DELETE | `/repos/{name}` | supprime (409 si un job de miroir vit ou si une PR est ouverte) |
| GET | `/repos/{name}/prs` | liste des PR à l'échelle du dépôt |
| POST/GET | `/repos/{name}/runs` | enregistrements des exécutions CI (voir [Enregistrements des exécutions CI](../guide/ci-runs.md)) |

## prs — pull requests

| Méthode | Chemin | Description |
|---|---|---|
| GET | `/prs?state=&base=&repo=&limit=&offset=` | liste globale (`state` ∈ open/closed/merged) |
| POST | `/prs` | crée (title, repo, base, head, etc.) |
| GET | `/prs/{number}` | une seule |
| POST | `/prs/{number}/merge` | fusionne (squash/avance rapide ; conflit = 409 avec la liste des conflits) |

Variante à l'échelle du dépôt : `GET /repos/{name}/prs` — le chemin détermine le dépôt ; si le filtre `?repo=` le contredit, 400.

## users — utilisateurs et rôles (admin)

| Méthode | Chemin | Description |
|---|---|---|
| GET | `/users` | liste (admin) |
| POST | `/users` | crée/met à jour (même nom = rotation du token ou changement de rôle ; `created_us` conservé) |
| GET | `/users/{name}` | un seul (admin) |
| DELETE | `/users/{name}` | supprime (admin, irréversible) |

Rôles : `admin` (tout) / `operator` (lecture-écriture) / `reader` (lecture seule). Champs et validations : voir
[Permissions et utilisateurs](../guide/rbac.md).

## mirror — tâches de miroir

| Méthode | Chemin | Description |
|---|---|---|
| GET | `/mirror/jobs` | liste toutes les tâches (avec `last_status` / `last_lane` / `last_detail`) |
| POST | `/mirror/jobs` | crée/met à jour (même nom = réinitialisation complète après validation) |
| DELETE | `/mirror/jobs/{name}` | supprime la tâche |
| POST | `/mirror/run` | déclenchement manuel (corps `{"job":"<name>"}`, 202 exécution asynchrone) |

Champs, validations et sémantique des voies : voir [Miroir et reflux](../guide/mirror.md).

## git-http — protocole intelligent

`/git/{repo}/*` : clone/fetch (upload-pack) et push (receive-pack). Authentification, détails de comportement et
limites : voir [Protocole git-http](../guide/git-http.md).

## Conventions générales

- `limit` des endpoints de liste : 50 par défaut, plafond 1000 (`limit=0` → 400) ;
- les paramètres de pagination sont **en ligne** dans la chaîne de requête (pas d'objet d'enrobage imbriqué) ;
- les champs attribués par le serveur (comme `number`, `started_us`) sont fixés par le serveur ; une valeur fournie par le client est écrasée ;
- les erreurs ne divulguent pas de détails internes (forme constante `internal server error`) ; les détails vont dans les journaux du serveur.
