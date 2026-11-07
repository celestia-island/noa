# Enregistrements des exécutions CI (API runs)

noa embarque un stockage d'enregistrements d'exécutions CI : un « pipeline de vérifications exécuté sur le head d'un dépôt donné » constitue une run. C'est la source de données de la voie `CI_BACKEND=noa` de la vue CI du panneau DevOps de chest.

## Endpoints

| Méthode | Chemin | Description |
|---|---|---|
| POST | `/api/v1/repos/{name}/runs` | enregistre une run (rôle en écriture) |
| GET | `/api/v1/repos/{name}/runs?limit=&offset=` | liste, les plus récentes d'abord |

- `{name}` doit exister dans le catalog (sinon 404) ;
- `number` est attribué par le serveur, dépôt par dépôt (atomique : balayage et écriture dans la même transaction d'écriture ; des enregistrements concurrents ne s'écrasent pas mutuellement leur numéro) ;
- validation : `head_sha` 40 hex ; `status` ∈ `queued | running | success | failure | cancelled` ;
  `kind` 1..=64 caractères ; `actor` 1..=200 ; `log_ref` / `summary` ≤ 2000.

## Enregistrer une run

```bash
curl -X POST -H "Authorization: Bearer $TOKEN" -H "Content-Type: application/json" \
  -d '{
    "head_sha": "<40-hex>",
    "kind": "basic-check",
    "status": "success",
    "actor": "farm-runner-1",
    "log_ref": "https://ci.example.com/logs/123"
  }' \
  http://127.0.0.1:3016/api/v1/repos/noa/runs
```

Réponse (201) : le serveur complète `repo`, `number`, `started_us` (quand le client ne les fournit pas).

## Interroger

```bash
curl -H "Authorization: Bearer $TOKEN" \
  "http://127.0.0.1:3016/api/v1/repos/noa/runs?limit=10&offset=0"
```

Les lignes renvoyées ressemblent à :

```json
{
  "repo": "noa",
  "number": 1,
  "head_sha": "…",
  "kind": "basic-check",
  "status": "success",
  "actor": "farm-runner-1",
  "started_us": 1794049465562618
}
```

## Bornes de conception

- **Les journaux ne sont pas stockés dans noa** : l'enregistrement porte un pointeur opaque `log_ref`, qui pointe vers le stockage de journaux du pipeline d'ordonnancement ;
- **Le contenu des lignes est plafonné** (voir validation) — pour empêcher un écrivain authentifié de gonfler le volume des lignes ;
- **Stockage** : table redb `runs`, clé `{repo}#{number:020}` (ordre lexicographique = ordre chronologique dans le dépôt).

## Intégration chest

Le gestionnaire `ci_ops` de chest lit la variable d'environnement `CI_BACKEND` :

- `github` (défaut) : voie v1 GitHub Actions, comportement strictement identique aux versions historiques ;
- `noa` : lit l'API runs de noa désignée par `NOA_API_BASE` + `NOA_API_TOKEN` et normalise les lignes vers la même forme de panneau que les lignes GitHub (`kind`→name, `number`→run_number, `started_us`→RFC3339
  `created_at`, états finaux mappés sur conclusion) ; la liste de watch utilise `CI_NOA_REPOS` (noms de répertoire à un seul segment), avec repli sur le dernier segment de `CI_GITHUB_REPOS` si elle n'est pas définie.

Le `ci.config.get` du panneau rapporte le champ `backend` ; `configured` dépend du backend — un déploiement pure-noa (sans token GitHub) est considéré comme connecté dès que le triplé noa est au complet.
