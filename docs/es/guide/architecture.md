# Visión general de la arquitectura

noa es un sistema de control de versiones distribuido nativo para agentes. No es «un envoltorio de git»: el modelo de objetos del núcleo incorpora direccionamiento por contenido SHA-256 y, a la vez, es compatible byte a byte con el álgebra de objetos de git (el mismo contenido de archivo produce el mismo OID de blob en noa y en git — la verificación de fidelidad del espejo depende precisamente de eso).

## El modelo de tres capas

```
工作区（Workspace）   —— 每个智能体一个隔离工作区，互不可见
   │ head 指向
快照（Snapshot）      —— 内容寻址的提交（SHA-256），DAG 结构（可多父 = 合并）
   │ tree_hash 指向
对象（ObjectStore）   —— blob / tree / symlink / gitlink，git 对象代数兼容
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

## Compatibilidad de objetos git

El import y el reflujo parten de que «el álgebra de objetos de git no cambia»:

- blob: el contenido bruto es el cuerpo del blob de git (`git_blob_oid(content)` igual que en git);
- árbol: el import concilia cada entrada por ruta/kind/oid (verificación de fidelidad);
- commit: la cadena de padres la mantienen las olas del espejo (`noa_<sha>` direccionado por contenido + enlace de padres).

Por eso el push de reflujo noa → git no necesita ninguna capa de conversión: los objetos de git del clon scratch se corresponden uno a uno con los objetos de noa.

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
