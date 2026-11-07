# Protocolo git-http

`/git/{repo}/*` implementa el protocolo HTTP inteligente: la ruta de lectura maneja directamente `git upload-pack --stateless-rpc` y la ruta de escritura (desde W2a2) maneja directamente `git receive-pack --stateless-rpc` — sin pasar por el envoltorio CGI de `git-http-backend`, por lo que la negociación del protocolo, las variables de entorno y la forma de los errores quedan totalmente controladas.

## Endpoints

| Método | Ruta | Uso |
|---|---|---|
| GET | `/git/{repo}/info/refs?service=git-upload-pack` | anuncio de clone/fetch |
| POST | `/git/{repo}/git-upload-pack` | negociación del pack |
| GET | `/git/{repo}/info/refs?service=git-receive-pack` | anuncio de push |
| POST | `/git/{repo}/git-receive-pack` | recepción del push |

`{repo}` debe ser un nombre de tarea de espejo — el cuerpo servido es precisamente el clon desnudo scratch de esa tarea
(`NOA_MIRROR_SCRATCH_DIR/{repo}.git`). Un repositorio inexistente devuelve 404.

## Detalles de comportamiento

- **Negociación de protocolo**: la cabecera `Git-Protocol` del cliente se pasa tal cual al subproceso (v2 se activa a petición; vacío = v0 por defecto). Esta es justamente la razón por la que la implementación directa sustituye a git-http-backend — el envoltorio CGI descarta esa cabecera en la ruta POST, degrada en silencio a v0 y rompe el clon.
- **Preámbulo del anuncio**: pkt-line `# service=` (cabecera de longitud calculada dinámicamente según el nombre del servicio) + flush; las respuestas RPC no llevan preámbulo.
- **Content-Type**: `application/x-git-{upload,receive}-pack[-advertisement]` estrictamente diferenciados.
- **Cuerpo de la petición**: el cuerpo del POST se escribe directamente en el stdin del subproceso; `Content-Encoding: gzip` se descomprime automáticamente
  (ratio de descompresión > 1000:1 rechazado — protección contra bombas; límites de 50 MiB por petición / 200 MiB descomprimidos).
- **Tiempos de espera**: 60 s de reloj de pared para todo el intercambio; al agotarse se mata el subproceso (kill_on_drop contra huérfanos que retienen bloqueos).
- **Concurrencia**: un semáforo a nivel de ruta limita los intercambios de git en vuelo (cota de memoria 4×50 MiB).
- **Entorno del subproceso**: tras `env_clear` solo se conservan `PATH` + `GIT_PROTOCOL` — una fuga de entorno es imposible en tiempo de compilación.

## Semántica del push

El push aterriza en las referencias locales del scratch; la ola del espejo después:

1. importa las referencias avanzadas del scratch al sistema de referencias de noa (instantáneas/espacios de trabajo);
2. con `backflow` activado, empuja el estado al origen.

El push no necesita ningún hook especial — las tres vías (lectura, recepción, reflujo) comparten el mismo scratch y el circuito converge automáticamente. El push exige una identidad autenticada; sin credenciales 401 + challenge Basic, rol reader 403.

## Límites conocidos

- El push no ofrece semántica de borrado (un borrado del lado de noa no se devuelve al origen — asimetría conocida).
- El dumb HTTP (`/info/refs` como archivo plano) no se sirve.
- El reflujo de los PR heads aterriza en `refs/noa/pr/<N>` (GitHub rechaza pushes de `refs/pull/*`).
