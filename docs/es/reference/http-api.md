# Referencia de la API HTTP

Base: `http://<host>:<port>/api/v1`. Todos los endpoints exigen autenticación bearer (`Authorization: Bearer
<token>`); no autenticado = 401 + challenge Basic. Aplicación de roles: escritura de reader = 403; `/users` fuera de
GET solo admin.

La forma de error está unificada en `{"error": "..."}`; paginación en línea (`?limit=` 1..=1000, `?offset=`).

## repos — catálogo de repositorios

| Método | Ruta | Descripción |
|---|---|---|
| GET | `/repos` | lista el catalog |
| POST | `/repos` | registra (`name` obligatorio, misma forma que el nombre de un job de espejo) |
| GET | `/repos/{name}` | uno solo |
| DELETE | `/repos/{name}` | borra (409 si vive un job de espejo o hay una PR abierta) |
| GET | `/repos/{name}/prs` | lista de PRs del repo |
| POST/GET | `/repos/{name}/runs` | registros de ejecuciones de CI (ver [Registros de ejecuciones de CI](../guide/ci-runs.md)) |

## prs — pull requests

| Método | Ruta | Descripción |
|---|---|---|
| GET | `/prs?state=&base=&repo=&limit=&offset=` | lista global (`state` ∈ open/closed/merged) |
| POST | `/prs` | crea (title, repo, base, head, etc.) |
| GET | `/prs/{number}` | una sola |
| POST | `/prs/{number}/merge` | fusiona (squash/avance rápido; conflicto = 409 con la lista de conflictos) |

Variante del repo: `GET /repos/{name}/prs` — la ruta determina el repo; si el filtro `?repo=` lo contradice, 400.

## users — usuarios y roles (admin)

| Método | Ruta | Descripción |
|---|---|---|
| GET | `/users` | lista (admin) |
| POST | `/users` | crea/actualiza (mismo nombre = rotación de token o cambio de rol; `created_us` se conserva) |
| GET | `/users/{name}` | uno solo (admin) |
| DELETE | `/users/{name}` | borra (admin, irreversible) |

Roles: `admin` (todo) / `operator` (lectura y escritura) / `reader` (solo lectura). Campos y validaciones: ver
[Permisos y usuarios](../guide/rbac.md).

## mirror — tareas de espejo

| Método | Ruta | Descripción |
|---|---|---|
| GET | `/mirror/jobs` | lista todas las tareas (con `last_status` / `last_lane` / `last_detail`) |
| POST | `/mirror/jobs` | crea/actualiza (mismo nombre = restablecimiento completo tras validar) |
| DELETE | `/mirror/jobs/{name}` | borra la tarea |
| POST | `/mirror/run` | disparo manual (cuerpo `{"job":"<name>"}`, 202 ejecución asíncrona) |

Campos, validaciones y semántica de las vías: ver [Espejo y reflujo](../guide/mirror.md).

## git-http — protocolo inteligente

`/git/{repo}/*`: clone/fetch (upload-pack) y push (receive-pack). Autenticación, detalles de comportamiento y
límites: ver [Protocolo git-http](../guide/git-http.md).

## Convenciones generales

- `limit` de los endpoints de lista: 50 por defecto, tope 1000 (`limit=0` → 400);
- los parámetros de paginación van **en línea** en la cadena de consulta (sin objeto envolvente anidado);
- los campos que asigna el servidor (como `number`, `started_us`) los fija el servidor; un valor proporcionado por el cliente se sobrescribe;
- los errores no filtran detalles internos (forma constante `internal server error`); los detalles van a los logs del servidor.
