# Espejo y reflujo

El sistema de espejo incorpora a noa el master, todos los tags y los PR heads activos de los repositorios externos (GitHub, etc.); la vía de reflujo empuja de vuelta el estado de noa al origen — formando la transición «noa como autoridad + GitHub como espejo», mientras farm/CNB siguen haciendo fetch de GitHub sin ningún cambio.

## Tarea de espejo (MirrorJob)

```json
{
  "name": "noa",
  "remote_url": "https://github.com/celestia-island/noa.git",
  "github_slug": "celestia-island/noa",
  "branches": ["master"],
  "tags": true,
  "pr_heads": true,
  "backflow": true,
  "canary_push": true,
  "enabled": true
}
```

| Campo | Descripción |
|---|---|
| `name` | `^[a-z0-9][a-z0-9-]{0,63}$`; es a la vez nombre del directorio scratch, nombre del repo en el catalog y segmento de la ruta de clonado |
| `remote_url` | solo se permiten `https://` / `file://`; credenciales incrustadas prohibidas (`@`, `?`) |
| `github_slug` | validación de forma `owner/repo`; lo usan el bypass de la Git Data API y el reflujo |
| `branches` | al menos una; se rechazan `tags`/`tags/*`, `pr/*` (conflicto del espacio de nombres de PR) y las formas ilegales de espacios de trabajo derivados |
| `tags` | espeja todos los tags |
| `pr_heads` | espeja los PR heads activos (`refs/pull/<N>/head` → `mirror/{job}/pr/<N>`) |
| `backflow` | empuja a cada ola el estado del scratch de vuelta al origen (ver más abajo) |
| `canary_push` | empuja una referencia sonda para validar la dirección de escritura |
| `enabled` | desactivada, el planificador la salta |

CRUD: `POST/GET /api/v1/mirror/jobs`, `DELETE /api/v1/mirror/jobs/{name}`,
`POST /api/v1/mirror/run` (disparo manual, cuerpo `{"job":"<name>"}`).

## Doble vía

- **Vía git**: clon desnudo scratch + `git fetch` — los tags, los PR heads y los pushes de reflujo pasan todos por ella.
- **Vía API**: cuando el transporte de git muere (la familia de las sacudidas de red), degrada automáticamente a import blob a blob por la Git Data API, verificando el SHA de cada blob (la fidelidad no se degrada según la vía).

La degradación se juega a escala de ola: fallo de `ensure_scratch` → la ola va por la vía API → la autocuración borra el scratch medio roto y la siguiente ola vuelve a clonar. El push de reflujo solo se ejecuta cuando la vía git está disponible (la API no sabe empujar referencias).

## PR heads

Con `pr_heads: true`, cada ola descarga además `refs/pull/<N>/head` y lo importa como
`mirror/{job}/pr/<N>` + un espacio de trabajo homónimo. El espacio de nombres de PR y el de ramas (ramas con forma `pr/<x>`) son mutuamente excluyentes; el upsert lo rechaza.

## Reflujo (backflow)

```text
scratch 本地引用 ──atomic push──▶ origin
  +refs/heads/*:refs/heads/*
  +refs/tags/*:refs/tags/*
  +refs/pull-heads/*:refs/noa/pr/*     （仅当 scratch 存在 pull-heads）
```

- **Credenciales**: el token resuelto desde `token_env` (`NOA_MIRROR_GH_TOKEN` por defecto) se incrusta como `x-access-token:<token>` en una URL de push de un solo uso; el token no queda en la configuración del scratch.
- **Semántica de fuerza**: una vez invertido el sentido, noa es la autoridad de escritura y las referencias del origen convergen hacia el scratch. La asimetría de los borrados (un borrado del lado de noa no borra la referencia del lado del origen) es una limitación conocida.
- **Semántica de fallo**: un fallo de reflujo pone la ola en FAILED — es la vía crítica de la aceptación y debe quedar bien a la vista.
- **Puente de espacios de nombres**: GitHub rechaza pushes de `refs/pull/*`, así que los PR heads aterrizan en
  `refs/noa/pr/<N>`.

## Sonda canary

Con `canary_push: true`, cada ola empuja al origen una referencia sonda `noa-mirror-canary` (el commit de la primera rama replicada), primero por el transporte de git y con degradación a la Git Data API si falla — sirve para validar continuamente las credenciales de la dirección de escritura y la conectividad. Un fallo de la sonda hace que la ola falle de forma sonora.

## Verificación de fidelidad

Cada commit importado se concilia entrada por entrada: el conjunto de rutas, el kind de las entradas y el OID de los blobs (`git_blob_oid` igual que en git) deben coincidir exactamente; cualquier diferencia hace fallar esa entrada y entra en el detalle de errores de la ola. El reflujo depende de este invariante — los objetos de noa y los de git se corresponden uno a uno, ningún push necesita capa de conversión.

## Limpieza de referencias caducadas (reconcile)

El conjunto aguas arriba de tags y PR heads es el conjunto de reconcile: las referencias que ya no aparecen aguas arriba se purgan de noa (espacios de nombres `mirror/{job}/tags/`, `mirror/{job}/pr/`). El push de tags del reflujo también se rige por lo que hay aguas arriba (un borrado del lado de noa no borra del lado del origen — asimetría conocida).

## Planificación

Con `NOA_MIRROR_ENABLED=1`, el planificador ejecuta todas las tareas enabled cada 900 segundos; `POST /mirror/run` permite el disparo manual. El detalle de las olas (vía, resultado de cada tarea, estado backflow/canary) está en `last_detail` / `last_lane` de
`GET /api/v1/mirror/jobs`.
