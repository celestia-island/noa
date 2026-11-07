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
export NOA_MIRROR_SCRATCH_DIR=/var/lib/noa/mirror-cache   # 镜像 scratch 根目录
export NOA_MIRROR_ENABLED=1                               # 启用镜像调度（默认 900s）
export NOA_MIRROR_GH_TOKEN="<github-pat>"                 # 可选：回流 / canary 推送凭据
noa-server --host 0.0.0.0 --port 3016
```

Tras el arranque:

```bash
curl -H "Authorization: Bearer <your-token>" http://127.0.0.1:3016/api/v1/repos
```

Una respuesta `[]` significa que el servicio está listo. Todos los endpoints REST exigen autenticación bearer; lo mismo vale para las rutas de lectura de `/git/{repo}/*` (una petición no autenticada recibe un 401 y un challenge `Basic`).

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

## Primer push (escribir la autoridad)

El push va por la ruta receive-pack de `/git/{repo}` y requiere una identidad autenticada:

```bash
cd hello
echo "hello noa" > README.md
git add -A && git commit -m "hello noa"
git push origin master
```

El push aterriza en el scratch del espejo; la siguiente ola del espejo lo incorpora automáticamente al sistema de instantáneas/referencias de noa y, si el job tiene `backflow` activado, también empuja el estado de vuelta al origen (GitHub) — esa es la forma de transición «noa como autoridad + GitHub como espejo».

## Siguientes pasos

- [Visión general de la arquitectura](./architecture.md) — modelo de espacios de trabajo / instantáneas / referencias
- [Espejo y reflujo](./mirror.md) — doble vía, PR heads, reflujo y canary
- [Referencia de la API HTTP](../reference/http-api.md) — todos los endpoints
