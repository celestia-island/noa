# Protocole git-http

`/git/{repo}/*` implémente le protocole HTTP intelligent : le chemin de lecture pilote directement `git upload-pack --stateless-rpc`, le chemin d'écriture (depuis W2a2) pilote directement `git receive-pack --stateless-rpc` — sans passer par l'enrobage CGI de `git-http-backend`, ce qui rend la négociation de protocole, les variables d'environnement et la forme des erreurs entièrement maîtrisées.

## Endpoints

| Méthode | Chemin | Usage |
|---|---|---|
| GET | `/git/{repo}/info/refs?service=git-upload-pack` | annonce clone/fetch |
| POST | `/git/{repo}/git-upload-pack` | négociation du pack |
| GET | `/git/{repo}/info/refs?service=git-receive-pack` | annonce push |
| POST | `/git/{repo}/git-receive-pack` | réception du push |

`{repo}` doit être un nom de tâche de miroir — le corps servi est précisément le clone nu scratch de cette tâche
(`NOA_MIRROR_SCRATCH_DIR/{repo}.git`). Un dépôt inexistant renvoie 404.

## Détails de comportement

- **Négociation de protocole** : l'en-tête `Git-Protocol` du client est transmis tel quel au sous-processus (v2 activé à la demande ; vide = v0 par défaut). C'est précisément la raison pour laquelle l'implémentation directe remplace git-http-backend — l'enrobage CGI abandonne cet en-tête sur le chemin POST, dégrade silencieusement en v0 et casse le clone.
- **Préambule d'annonce** : pkt-line `# service=` (en-tête de longueur calculé dynamiquement selon le nom du service) + flush ; les réponses RPC sont sans préambule.
- **Content-Type** : `application/x-git-{upload,receive}-pack[-advertisement]` strictement distingués.
- **Corps de requête** : le corps du POST est écrit directement sur le stdin du sous-processus ; `Content-Encoding: gzip` est décompressé automatiquement
  (ratio de décompression > 1000:1 refusé — protection contre les bombes ; plafonds de 50 MiB par requête / 200 MiB décompressés).
- **Délais d'attente** : 60 s de temps mural pour tout l'échange ; à l'expiration, le sous-processus est tué (kill_on_drop contre les orphelins qui gardent des verrous).
- **Concurrence** : un sémaphore au niveau des routes limite le nombre d'échanges git en vol (borne mémoire 4×50 MiB).
- **Environnement du sous-processus** : après `env_clear`, seuls `PATH` + `GIT_PROTOCOL` sont conservés — une fuite d'environnement est impossible à la compilation.

## Sémantique du push

Le push atterrit dans les références locales du scratch ; la vague de miroir fait ensuite :

1. l'import des références avancées du scratch dans le système de références de noa (instantanés/espaces de travail) ;
2. si `backflow` est activé, le push de l'état vers l'origine.

Le push n'exige donc aucun hook particulier — les trois voies (lecture, réception, reflux) partagent le même scratch et la boucle converge automatiquement. Le push exige une identité authentifiée ; sans identifiants 401 + challenge Basic, rôle reader 403.

## Limites connues

- Le push ne fournit pas de sémantique de suppression (une suppression côté noa n'est pas renvoyée à l'origine — asymétrie connue).
- Le dumb HTTP (`/info/refs` en fichier brut) n'est pas servi.
- Le reflux des PR heads atterrit sous `refs/noa/pr/<N>` (GitHub refuse les pushes `refs/pull/*`).
- Les pushes de reflux ont pour source les objets git du scratch — les **commits natifs de noa** (instantanés purs d'espace de travail n'ayant jamais transité par le scratch) sont hors du périmètre du reflux tant que le sérialiseur instantané → objets git n'existe pas (voir [Architecture](./architecture.md)).
