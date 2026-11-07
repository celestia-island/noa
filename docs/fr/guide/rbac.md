# Permissions et utilisateurs

L'authentification de noa suit le principe « un seul token, deux canaux » : REST et git partagent les mêmes identifiants, mais les droits d'écriture sont appliqués séparément sur chaque canal.

## Identifiants

- **Token d'amorçage** : variable d'environnement `NOA_API_TOKEN`. Le serveur refuse de démarrer si elle est vide (chaîne vide ou ne contenant que des espaces = refus de démarrage) ; ce token est l'identité admin.
- **Tokens des utilisateurs enregistrés** : créés via REST (voir plus bas), stockés hachés en sha256, comparés en temps constant.

Formes d'authentification (identiques sur les deux canaux) :

```text
Authorization: Bearer <token>
Authorization: Basic <base64(any-user:token)>     # git client convention: any username
```

Un serveur à token vide (`NOA_API_TOKEN=""`) ne dégénère pas en lecture anonyme — le démarrage est refusé immédiatement, et la garde de non-vide avant comparaison constitue une double défense.

## Rôles

| Rôle | Lecture | Écriture (REST / git push) | Administration (CRUD des utilisateurs) |
|---|---|---|---|
| admin | ✓ | ✓ | ✓ |
| operator | ✓ | ✓ | ✗ |
| reader | ✓ | ✗ | ✗ |

- REST : application centralisée par le middleware — les requêtes d'écriture de reader reçoivent 403 ; `/api/v1/users` hors GET reçoit 403 (admin uniquement).
- git : le push (receive-pack) exige can_write — sans identifiants 401 (avec challenge Basic, sur quoi le client git invite à s'identifier) ; avec identifiants mais rôle reader, 403 (indiquant qu'il faut le rôle write). Le clone/fetch est ouvert à toute identité authentifiée.

## REST de gestion des utilisateurs

```bash
# Create (admin)
curl -X POST -H "Authorization: Bearer $ADMIN" -H "Content-Type: application/json" \
  -d '{"name":"alice","role":"operator","token":"<alice-token>"}' \
  http://127.0.0.1:3016/api/v1/users

# List (admin) / single
curl -H "Authorization: Bearer $ADMIN" http://127.0.0.1:3016/api/v1/users
curl -H "Authorization: Bearer $ADMIN" http://127.0.0.1:3016/api/v1/users/alice

# Delete (admin)
curl -X DELETE -H "Authorization: Bearer $ADMIN" \
  http://127.0.0.1:3016/api/v1/users/alice
```

Règles :

- `name` et `role` (admin/operator/reader) sont obligatoires ; le serveur hache le token en sha256 avant de le stocker.
- La mise à jour (POST du même nom au même endpoint) permet de faire tourner le token ou de changer le rôle ; `created_us` conserve sa valeur d'origine.
- La suppression est réservée à l'admin et irréversible.

## Exemple du canal git

```bash
# Read (any authenticated identity; public repos work with a placeholder username)
git clone http://<token>@noa.example.com:3016/git/noa

# Write (requires operator/admin)
git push origin master
```

Un push non authentifié reçoit 401 + challenge Basic (le client git en déduit l'invite d'identifiants) ; un push sous identité reader reçoit
403 (indiquant qu'il faut le rôle write).
