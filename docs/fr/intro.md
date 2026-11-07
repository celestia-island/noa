<p align="center"><img src="https://raw.githubusercontent.com/celestia-island/docs.celestia.world/dev/res/logo/noa.webp" alt="noa" width="200" /></p>

<h1 align="center">Noa</h1>

<p align="center"><strong>Système de contrôle de versions distribué natif pour les agents</strong></p>

<div align="center">

[![License](https://img.shields.io/badge/license-SySL%201.0-blue)](../../LICENSE)
[![GitHub](https://img.shields.io/badge/github-celestia--island%2Fnoa-blue.svg)](https://github.com/celestia-island/noa)

</div>

<div align="center">

[English](../en/intro.md) ·
[简体中文](../zh-Hans/intro.md) ·
[繁體中文](../zh-Hant/intro.md) ·
[日本語](../ja/intro.md) ·
[한국어](../ko/intro.md) ·
**Français** ·
[Español](../es/intro.md) ·
[Русский](../ru/intro.md) ·
[العربية](../ar/intro.md)

</div>

Espaces de travail isolés par agent, instantanés adressés par contenu SHA-256, serveur complet du protocole intelligent de git (clone/fetch/push), forge auto-hébergée avec miroir et reflux intégrés, fusion tri-directionnelle au niveau des lignes avec détection précise des renommages, RBAC multi-utilisateur, API d'enregistrement des exécutions CI — le tout intégré dans un binaire unique + stockage embarqué.

## Documentation

La documentation française complète se trouve dans `docs/fr/` de ce dépôt (ce site est publié en synchro sur
[noa.docs.celestia.world](https://noa.docs.celestia.world)) :

| Documentation | Contenu |
|---|---|
| [Démarrage rapide](./guide/getting-started.md) | compilation, démarrage du serveur, premier clone/push |
| [Vue d'ensemble de l'architecture](./guide/architecture.md) | modèle espaces de travail / instantanés / références, composants serveur |
| [Miroir et reflux](./guide/mirror.md) | double voie, PR heads, reflux et canary, vérification de fidélité |
| [Permissions et utilisateurs](./guide/rbac.md) | rôles, tokens, application des règles sur les canaux REST et git |
| [Protocole git-http](./guide/git-http.md) | endpoints du protocole intelligent, sémantique du push, négociation du protocole |
| [Fusion et conflits](./guide/merge.md) | diff3 au niveau des lignes, renommage précis, sémantique de repli |
| [Enregistrements des exécutions CI](./guide/ci-runs.md) | API runs, intégration chest |
| [Référence de l'API HTTP](./reference/http-api.md) | tous les endpoints REST |
| [Référence CLI](./reference/cli.md) | panorama de la ligne de commande |
| [Déploiement](./reference/deploy.md) | systemd, variables d'environnement, trio de déploiement |

La version anglaise se trouve dans `docs/en/` (synchronisée par lots depuis le chinois simplifié).

Code source : [noa](https://github.com/celestia-island/noa).
