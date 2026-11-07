# Visión general de la arquitectura

noa es un sistema de control de versiones distribuido nativo para agentes. No es «un envoltorio de git»: el modelo de objetos del núcleo incorpora direccionamiento por contenido SHA-256 y, a la vez, es compatible byte a byte con el álgebra de objetos de git (el mismo contenido de archivo produce el mismo OID de blob en noa y en git — la verificación de fidelidad del espejo depende precisamente de eso).

## El modelo de tres capas

```
Workspace          — one isolated workspace per agent, invisible to each other
   │ head points to
Snapshot           — content-addressed commit (SHA-256), DAG structure (multiple parents = merge)
   │ tree_hash points to
ObjectStore        — blob / tree / symlink / gitlink (blobs byte-compatible with git;
                     trees/commits use noa's own SHA-256 addressing)
```

- **El espacio de trabajo** es la primitiva central de aislamiento de noa: varios agentes mantienen, dentro del mismo repositorio, un espacio de trabajo y un head propios e independientes, sin pisarse entre sí. Los espacios de nombres del sistema como `mirror.<job>.<branch>` también existen en forma de espacio de trabajo.
- **La instantánea** es el equivalente de un commit: `tree_hash` + `parents[]` + `author` + `timestamp` + `message`. El ID de instantánea tiene la forma `noa_<sha256>` (direccionamiento por contenido; importar repetidamente el mismo hash de árbol es idempotente).
- **Las referencias** (RedbRefStore): `HEAD`, `mirror/{job}/{branch}`, `mirror/{job}/tags/{tag}`, `mirror/{job}/pr/{N}`, etc. son todas filas de referencias en redb que apuntan a IDs de instantáneas.

El motor de almacenamiento es un KV embebido de archivo único (redb) — desplegar equivale a copiar un directorio, sin base de datos externa.

## Componentes del servidor (noa-server)

| Componente | Responsabilidad |
|---|---|
| API REST (`/api/v1/*`) | CRUD de repos / PRs / runs / users / mirror-jobs |
| git-http (`/git/{repo}/*`) | smart HTTP: upload-pack (clone/fetch) + receive-pack (push, desde W2a2) |
| Motor de espejo | olas programadas: importa a noa el master / tags / PR heads traídos del origen; puede empujar reflujo |
| run-records | almacenamiento de registros de ejecuciones de CI (fuente de datos del panel DevOps de chest) |
| Middleware de autenticación | autenticación bearer a nivel de ruta + aplicación de roles (REST) y de can_write (git push) |

Todo el estado del servidor cabe en una única base redb; el servidor queda reducido a «el directorio es todo».

## El límite preciso de la compatibilidad con git

La compatibilidad es por capas — enunciada con exactitud:

- **blob: compatible byte a byte.** El contenido bruto ES el cuerpo del blob de git; `git_blob_oid(content)` coincide con git (la verificación de fidelidad del espejo concilia blob por blob precisamente gracias a esto);
- **árbol: semánticamente equivalente, los IDs no.** Rutas/kinds/OID de cada blob corresponden exactamente, pero los IDs de árbol de noa usan su propio direccionamiento por contenido SHA-256, no el formato de objeto árbol de git;
- **commit: modelo propio de noa.** Los IDs de instantánea tienen la forma `noa_<sha256>`, no son objetos commit de git.

La representación en forma git vive en el **repositorio git real del scratch del espejo**. El push de reflujo usa los objetos git del scratch directamente (conversión cero para el contenido importado de git); **el contenido nativo de noa** (instantáneas puras de espacio de trabajo que nunca pasaron por el scratch) no tiene forma de objeto git — refluirlo directamente requiere un serializador de instantánea → objetos git. Es una limitación conocida (en la implementación actual tales referencias no aparecen en los pushes de reflujo).

## El papel del scratch del espejo

`NOA_MIRROR_SCRATCH_DIR/{job}.git` es el clon desnudo (bare) de cada tarea de espejo y cumple un triple papel:

1. **Caché de descarga** — las olas hacen fetch del origen; el importador lee aquí los objetos de git;
2. **Cuerpo servido por git-http** — `/git/{repo}` ejecuta directamente upload-pack / receive-pack sobre él;
3. **Origen de push del reflujo** — la vía de reflujo empuja atómicamente sus referencias locales al origen.

El push (receive-pack) actualiza las referencias locales del scratch; la siguiente ola lo importa al sistema de referencias de noa y la vía de reflujo lo empuja de vuelta al origen — tres rutas comparten el mismo scratch y el circuito se cierra solo.

## Índice de páginas

- [Inicio rápido](./getting-started.md)
- [Espejo y reflujo](./mirror.md)
- [Permisos y usuarios](./rbac.md)
- [Protocolo git-http](./git-http.md)
- [Fusión y conflictos](./merge.md)
- [Registros de ejecuciones de CI](./ci-runs.md)
- [Referencia de la API HTTP](../reference/http-api.md)
- [Referencia de la CLI](../reference/cli.md)
- [Despliegue](../reference/deploy.md)
