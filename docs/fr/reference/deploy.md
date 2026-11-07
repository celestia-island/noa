# Déploiement

Forme de déploiement de noa-server : binaire unique + base redb unique + unité systemd. Les instances de production sont supervisées par malkuth.

## Le trio de déploiement

1. **Artefacts** : le produit de `cargo build --release --locked --bin noa-server` est copié vers le chemin de déploiement
   (remplacement atomique : écrire d'abord `<bin>.new` puis renommer), avec vérification sha256 des deux côtés ;
2. **Nouvelles tentatives health** : après le démarrage, sondage de `/api/v1/*` (401 = processus vivant et authentification fonctionnelle) ;
3. **Retour arrière** : conserver le chemin de l'artefact de la version précédente, `mv` de retour + redémarrage.

## Variables d'environnement (configuration d'amorçage de classe A → fichier env 0600)

| Variable | Description |
|---|---|
| `NOA_API_TOKEN` | token admin d'amorçage ; **vide / ne contenant que des espaces = refus de démarrage** |
| `NOA_MIRROR_ENABLED` | interrupteur de planification du miroir |
| `NOA_MIRROR_INTERVAL_SECS` | intervalle entre vagues (900 par défaut) |
| `NOA_MIRROR_SCRATCH_DIR` | racine du scratch (ex. `/var/lib/noa/mirror-cache`) |
| `NOA_MIRROR_GH_TOKEN` | identifiants d'écriture GitHub du reflux / canary (nom par défaut de token_env) |
| `NOA_MIRROR_GH_TOKEN_ENV` | nom de la variable d'environnement qui remplace les identifiants (par défaut : la précédente) |

Les identifiants ne vont que sur le plan de déploiement (env 0600 / systemd LoadCredential), jamais dans la base, l'arbre git ou la documentation.

## Unité systemd

```ini
[Service]
EnvironmentFile=/etc/noa/noa-server.env
ExecStart=/usr/local/bin/noa-server --host 0.0.0.0 --port 3016
Restart=on-failure
```

- Écoute sur `0.0.0.0` (convention des nœuds de la famille : bearer sur toutes les routes + application du rôle en écriture, surface exposée sûre) ;
- Après modification de l'env : `systemctl daemon-reload && systemctl restart` ; vérifier que
  `tr '\0' '\n' < /proc/$MAINPID/environ` contient les nouvelles clés ;
- Les instances de production sont supervisées par malkuth (passer par l'exclusivité de claim avant de modifier le chemin de déploiement).

## Ports et domaines

L'attribution des ports se lit dans le registre du workspace (`_ledger/ports-and-domains.md`) ; l'instance de production est sur 3016, et nginx, en façade, y assure la terminaison TLS.

## Instances multiples

redb est une base à fichier unique — une instance, un fichier de base. Plusieurs instances = plusieurs répertoires et plusieurs ports (comme le mode double instance de `chest-demo` et du chest de production) ; le transport de configuration entre instances passe par RPC import / export, sans copie de fichiers de configuration.

## Vérifications après déploiement

```bash
# Auth and readiness
curl -s -o /dev/null -w '%{http_code}\n' http://127.0.0.1:3016/api/v1/prs   # expect 401
# Mirror wave
curl -X POST -H "Authorization: Bearer $T" -H "Content-Type: application/json" \
  -d '{"job":"noa"}' http://127.0.0.1:3016/api/v1/mirror/run               # expect 202
```

Détails du miroir et sémantique du reflux : voir [Miroir et reflux](../guide/mirror.md).
