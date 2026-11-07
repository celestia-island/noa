# Inicio rápido

Esta página lleva de cero a un noa en marcha: compilación, arranque del servidor y el primer clone / push.

## Requisitos previos

- Rust stable ≥ 1.99 (instalado con `rustup`)
- git ≥ 2.39 (el smart HTTP del servidor maneja directamente `upload-pack` / `receive-pack`)
- Opcional: node + pnpm (para compilar la WebUI)

## Compilación

```bash
git clone https://github.com/celestia-island/noa.git
cd noa
cargo build --release --locked
```

Productos: `target/release/noa` (CLI) y `target/release/noa-server` (servidor).

## Arrancar el servidor

La configuración mínima solo necesita un token de API (credencial de arranque de administración):

```bash
export NOA_API_TOKEN="<your-token>"
export NOA_MIRROR_SCRATCH_DIR=/var/lib/noa/mirror-cache   # mirror scratch root
export NOA_MIRROR_ENABLED=1                               # enable mirror scheduling (default 900s)
export NOA_MIRROR_GH_TOKEN="<github-pat>"                 # optional: back-flow / canary push credential
noa-server --host 0.0.0.0 --port 3016
```

Tras el arranque:

```bash
curl -H "Authorization: Bearer <your-token>" http://127.0.0.1:3016/api/v1/repos
```

Una respuesta `[]` significa que el servicio está listo. Todos los endpoints REST exigen autenticación bearer; lo mismo vale para las rutas de lectura de `/git/{repo}/*` (una petición no autenticada recibe un 401 y un challenge `Basic`).

## El flujo de trabajo nativo de noa (recomendado)

El CLI `noa` es la puerta de entrada de primera clase — lo que se clona es un repositorio
donde `.git` y `.noa` coexisten (git gestiona el código fuente, noa gestiona los datos de
los agentes):

```bash
noa clone http://<token>@127.0.0.1:3016/git/hello --path hello
cd hello
noa workspace create agent-a     # a per-agent isolated workspace
# … edit files …
noa snapshot create -m "change" -a agent-a
noa log                          # this workspace's snapshot history
noa push                         # publishes the workspace snapshot (serialized to a git commit)
```

`noa push` publica **el estado de la instantánea del espacio de trabajo** (genera un
commit `[noa export]`), no los commits de `.git`. Las dos vías son paralelas; no las
mezcles (ejecuta `noa snapshot create` antes de hacer push, o publicarás una instantánea
obsoleta). La autenticación se inyecta desde las variables de entorno `NOA_GIT_USER` +
`NOA_GIT_PASS`. El directorio completo (incluido `.git/noa`) se copia a otra máquina y
sigue funcionando.

## La vía de compatibilidad git (cualquier herramienta git sirve)

El mismo endpoint `/git/{repo}` también habla el smart HTTP estándar de git — git normal,
CI y farm funcionan sin cambios:

```bash
git clone http://<token>@127.0.0.1:3016/git/hello
```

Los push (receive-pack) requieren el rol de escritura; un push que aterriza **dispara
inmediatamente la ola de importación** (el reflujo continúa en la misma ola). Esta es la
forma de transición que mantiene a farm/CNB sin cambios.

## Primer espejo (incorporar un repositorio de GitHub a noa)

```bash
curl -X POST -H "Authorization: Bearer <your-token>" \
  -H "Content-Type: application/json" \
  -d '{
    "name": "hello",
    "remote_url": "https://github.com/you/hello.git",
    "github_slug": "you/hello",
    "branches": ["master"],
    "tags": true,
    "pr_heads": true,
    "enabled": true
  }' \
  http://127.0.0.1:3016/api/v1/mirror/jobs

curl -X POST -H "Authorization: Bearer <your-token>" \
  -H "Content-Type: application/json" \
  -d '{"job":"hello"}' \
  http://127.0.0.1:3016/api/v1/mirror/run
```

Cuando la ola termina, el repositorio ya se puede clonar a través de noa:

```bash
git clone http://<your-token>@127.0.0.1:3016/git/hello
```

## Copia de seguridad y migración

Todo el estado del servidor se exporta a un directorio portátil (véase
[Despliegue](../reference/deploy.md) § copia de seguridad): una instantánea consistente
de redb + un bundle git clonable por repositorio + un manifiesto sha256;
`noa-server --import-state <dir>` restaura sin conexión. El criterio de aceptación es el
simulacro: exportar → restaurar en otra máquina → servir → conciliar.


## Siguientes pasos

- [Visión general de la arquitectura](./architecture.md) — modelo de espacios de trabajo / instantáneas / referencias
- [Espejo y reflujo](./mirror.md) — doble vía, PR heads, reflujo y canary
- [Referencia de la API HTTP](../reference/http-api.md) — todos los endpoints
