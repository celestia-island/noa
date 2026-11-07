# Architecture

noa est un système de contrôle de versions distribué natif pour l'IA. Ce n'est pas un
« wrapper git » : le modèle d'objets central utilise son propre adressage de contenu
SHA-256 tout en restant compatible octet par octet avec l'algèbre d'objets de git (le
même contenu de fichier produit le même OID de blob dans noa et dans git — les
vérifications de fidélité du miroir reposent précisément là-dessus).

## Le modèle à trois couches

```
Workspace          — one isolated workspace per agent, invisible to each other
   │ head points to
Snapshot           — content-addressed commit (SHA-256), DAG structure (multiple parents = merge)
   │ tree_hash points to
ObjectStore        — blob / tree / symlink / gitlink, git object-algebra compatible
```

- **Les espaces de travail** sont la primitive d'isolation centrale de noa : plusieurs
  agents dans un même dépôt détiennent chacun leur espace de travail et leur head sans
  s'écraser mutuellement. Les espaces de noms système comme `mirror.<job>.<branch>`
  existent eux aussi comme espaces de travail.
- **Les instantanés (snapshots)** sont l'équivalent des commits : `tree_hash` +
  `parents[]` + `author` + `timestamp` + `message`. Les identifiants d'instantané
  ressemblent à `noa_<sha256>` (adressés par contenu ; les imports répétés d'un arbre
  inchangé sont idempotents).
- **Les références (refs)** (RedbRefStore) : `HEAD`, `mirror/{job}/{branch}`,
  `mirror/{job}/tags/{tag}`, `mirror/{job}/pr/{N}` — toutes les lignes de refs dans
  redb pointent vers des identifiants d'instantané.

Le moteur de stockage est une base KV embarquée à fichier unique (redb) — le
déploiement se résume à « copier le répertoire », sans base de données externe.

## Composants serveur (noa-server)

| Composant | Responsabilité |
|---|---|
| API REST (`/api/v1/*`) | CRUD des dépôts / PR / runs / utilisateurs / tâches de miroir |
| git-http (`/git/{repo}/*`) | HTTP intelligent : upload-pack (clone/fetch) + receive-pack (push, depuis W2a2) |
| Moteur de miroir | vagues planifiées : tirer master / tags / têtes de PR depuis l'origine vers noa ; push de retour optionnel |
| Enregistrements de runs | stockage des enregistrements de runs CI (source de données du panneau DevOps de chest) |
| Middleware d'authentification | authentification bearer au niveau des routes + application des rôles (REST) et contrôle can_write (git push) |

Tout l'état du serveur vit dans une seule base redb — « le répertoire est tout ».

## Compatibilité des objets git

L'import et la voie de retour supposent tous deux que « l'algèbre d'objets de git
reste inchangée » :

- blobs : le contenu brut EST le corps du blob git (`git_blob_oid(content)` correspond
  à git) ;
- trees : les imports réconcilient chemin/type/oid entrée par entrée (vérifications de
  fidélité) ;
- commits : la vague de miroir maintient le chaînage des parents (adressage de contenu
  `noa_<sha>` + arêtes parentales).

La voie de retour noa → git n'exige donc aucune couche de conversion : les objets git
du scratch sont utilisés directement — pour le contenu importé de git, aucun push de reflux n'a besoin de couche de conversion. **Limite** : le contenu natif de noa n'a pas de forme d'objet git — son reflux direct nécessite un sérialiseur instantané → objets git (limitation connue).

## Rôle du scratch du miroir

`NOA_MIRROR_SCRATCH_DIR/{job}.git` est un clone nu par tâche de miroir, avec trois
rôles :

1. **cache de source de tirage** — les vagues récupèrent (fetch) depuis l'origine ;
   l'importeur lit les objets git ici ;
2. **corps de service git-http** — `/git/{repo}` y exécute directement upload-pack /
   receive-pack ;
3. **source de push du retour** — la voie de retour repousse atomiquement ses refs
   locales vers l'origine.

Un push (receive-pack) met à jour les refs locales du scratch ; la vague suivante les
importe dans le système de références de noa et la voie de retour renvoie l'état vers
l'origine — trois chemins partagent un seul scratch, et la boucle se referme
d'elle-même.

## Index de la documentation

- [Prise en main](./getting-started.md)
- [Miroir et voie de retour](./mirror.md)
- [RBAC](./rbac.md)
- [Protocole git-http](./git-http.md)
- [Fusion et conflits](./merge.md)
- [Enregistrements de runs CI](./ci-runs.md)
- [Référence de l'API HTTP](../reference/http-api.md)
- [Référence CLI](../reference/cli.md)
- [Déploiement](../reference/deploy.md)
