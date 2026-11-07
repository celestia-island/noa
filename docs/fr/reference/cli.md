# Référence CLI

`noa` est la porte d'entrée en ligne de commande côté espace de travail (dérivé clap ; `noa --help` pour la signature à jour). Toutes les commandes s'exécutent dans un dépôt ayant subi un `noa init`.

## Liste des commandes

| Commande | Usage |
|---|---|
| `noa init` | initialise le répertoire courant en dépôt noa (crée le répertoire de métadonnées `.noa`) |
| `noa status` | état de l'espace de travail courant : head, changements non validés, liste des espaces de travail |
| `noa log` | historique des instantanés de l'espace de travail courant (rendu tronqué + parcours de la chaîne de parents) |
| `noa snapshot` | opérations sur les instantanés (création / consultation) |
| `noa workspace` | gestion des espaces de travail : `workspace create <name>` (isolation par agent), changement, liste |
| `noa remote` | gestion des distants (add / set-url / list) |
| `noa push` / `noa pull` / `noa fetch` / `noa clone` | synchronisation avec le distant (sémantique compatible git) |
| `noa resolve <path>` | résout un chemin imbriqué en entrée d'arbre |
| `noa coauthor` | ajoute des co-auteurs à un commit (signature de collaboration entre agents) |
| `noa hook` | gère les hooks du dépôt (avec marque sentinelle, écrasement sûr des hooks existants) |
| `noa verify` | ajoute un enregistrement `verify.run/1` au journal d'agent de l'espace de travail courant (JSON via stdin ou `--file`) — écrivain CLI du registre de vérification |
| `noa sync` | opérations de synchronisation |
| `noa storage` | opérations du backend de stockage |
| `noa pr` | cycle de vie des PR (indépendant du forge) : `pr create` / `pr list` / `pr diff`, etc. |

## Flux de travail avec isolation par espace de travail

```bash
noa init
noa workspace create agent-a      # agent A's isolated workspace
# … edit inside the workspace …
noa snapshot -m "agent-a's changes"
noa log                           # sees only this workspace's parent chain
```

Plusieurs agents font chacun leur `workspace create`, sans jamais voir le head ni le contenu des autres — c'est la différence fondamentale entre noa et le flux de branches traditionnel : l'unité d'isolation est l'espace de travail, pas la référence.

## Coopération avec le serveur

Le push/pull/fetch/clone du CLI interopère avec le protocole intelligent `/git/*` du serveur (sémantique compatible git). Pour un distant authentifié, les variables d'environnement `NOA_GIT_USER` + `NOA_GIT_PASS` injectent les identifiants (injection pour clone/push/pull ; le token ne sert qu'au transport et **n'est jamais écrit** dans `.git/noa/config`). Quand le distant pointe vers un noa-server :

```bash
noa remote add origin http://noa.example.com:3016/git/noa
noa push origin master
```

Le push passe par receive-pack et exige une identité authentifiée (le token fait office de mot de passe ; le rôle doit autoriser l'écriture). Notez que `noa push` publie **l'instantané courant de l'espace de travail** (sérialisé côté client en commit git `[noa export]`) — après vos modifications, exécutez d'abord `noa snapshot create` puis poussez ; pousser directement des changements non instantanés publie un instantané périmé.

## verify et le registre

`noa verify` est l'écrivain CLI du registre de vérification : les enregistrements sont ajoutés, selon le schema `verify.run/1`, au journal d'agent de l'espace de travail courant (JSONL, ajout seul). Un enregistrement peut porter une liaison `hardware=<hw-id>` (de pair avec l'audit par registre matériel côté espace de travail).

> Le registre faisant autorité est côté fichiers (`/mnt/work/verify-ledger/`) ; l'écriture CLI est une double écriture fail-open — laisser l'écrivain ouvert sans vérification de chaîne est une décision de conception ; la vérification de la chaîne incombe au lecteur.
