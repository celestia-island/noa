<p align="center"><img src="https://raw.githubusercontent.com/celestia-island/docs.celestia.world/dev/res/logo/noa.webp" alt="noa" width="200" /></p>

<h1 align="center">Noa</h1>

<p align="center"><strong>Sistema de control de versiones distribuido nativo para agentes</strong></p>

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
[Français](../fr/intro.md) ·
**Español** ·
[Русский](../ru/intro.md) ·
[العربية](../ar/intro.md)

</div>

Espacios de trabajo aislados por agente, instantáneas direccionadas por contenido SHA-256, servidor completo del protocolo inteligente de git (clone/fetch/push), forge autoalojado con espejo y reflujo integrados, fusión de tres vías a nivel de línea con detección precisa de renombrados, RBAC multiusuario, API de registros de ejecuciones de CI — todo integrado en un binario único + almacenamiento embebido.

## Documentación

La documentación completa en español está en `docs/es/` de este repositorio (este sitio se publica sincronizado en
[noa.docs.celestia.world](https://noa.docs.celestia.world)):

| Documentación | Contenido |
|---|---|
| [Inicio rápido](./guide/getting-started.md) | compilación, arranque del servidor, primer clone/push |
| [Visión general de la arquitectura](./guide/architecture.md) | modelo de espacios de trabajo / instantáneas / referencias, componentes del servidor |
| [Espejo y reflujo](./guide/mirror.md) | doble vía, PR heads, reflujo y canary, verificación de fidelidad |
| [Permisos y usuarios](./guide/rbac.md) | roles, tokens, aplicación de reglas en los canales REST y git |
| [Protocolo git-http](./guide/git-http.md) | endpoints del protocolo inteligente, semántica del push, negociación del protocolo |
| [Fusión y conflictos](./guide/merge.md) | diff3 a nivel de línea, renombrado preciso, semántica de degradación |
| [Registros de ejecuciones de CI](./guide/ci-runs.md) | API de runs, integración con chest |
| [Referencia de la API HTTP](./reference/http-api.md) | todos los endpoints REST |
| [Referencia de la CLI](./reference/cli.md) | panorama de la línea de comandos |
| [Despliegue](./reference/deploy.md) | systemd, variables de entorno, trío de despliegue |

La versión en inglés está en `docs/en/` (sincronizada por lotes desde el chino simplificado).

Código fuente: [noa](https://github.com/celestia-island/noa).
