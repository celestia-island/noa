# Referencia de la CLI

`noa` es la puerta de entrada de línea de comandos del lado del espacio de trabajo (derivado de clap; `noa --help` muestra la firma vigente). Todos los comandos se ejecutan dentro de un repo que haya pasado por `noa init`.

## Lista de comandos

| Comando | Uso |
|---|---|
| `noa init` | inicializa el directorio actual como repo de noa (crea el directorio de metadatos `.noa`) |
| `noa status` | estado del espacio de trabajo actual: head, cambios sin confirmar, lista de espacios de trabajo |
| `noa log` | historial de instantáneas del espacio de trabajo actual (render truncado + recorrido de la cadena de padres) |
| `noa snapshot` | operaciones de instantáneas (crear / ver) |
| `noa workspace` | gestión de espacios de trabajo: `workspace create <name>` (aislamiento por agente), cambio, lista |
| `noa remote` | gestión de remotos (add / set-url / list) |
| `noa push` / `noa pull` / `noa fetch` / `noa clone` | sincronización con el remoto (semántica compatible con git) |
| `noa resolve <path>` | resuelve una ruta anidada a una entrada del árbol |
| `noa coauthor` | añade coautores al commit (firma de colaboración entre agentes) |
| `noa hook` | gestiona los hooks del repo (con marca centinela, sobrescritura segura de los hooks existentes) |
| `noa verify` | añade un registro `verify.run/1` al log de agente del espacio de trabajo actual (JSON por stdin o `--file`) — escritor CLI del registro de verificación |
| `noa sync` | operaciones de sincronización |
| `noa storage` | operaciones del backend de almacenamiento |
| `noa pr` | ciclo de vida de PRs (independiente del forge): `pr create` / `pr list` / `pr diff`, etc. |

## Flujo de trabajo con aislamiento por espacio de trabajo

```bash
noa init
noa workspace create agent-a      # agent A's isolated workspace
# … edit inside the workspace …
noa snapshot -m "agent-a's changes"
noa log                           # sees only this workspace's parent chain
```

Varios agentes hacen cada uno su `workspace create`, sin ver jamás el head ni el contenido de los demás — esta es la diferencia fundamental entre noa y el flujo de ramas tradicional: la unidad de aislamiento es el espacio de trabajo, no la referencia.

## Cooperación con el servidor

El push/pull/fetch/clone de la CLI interoperates con el protocolo inteligente `/git/*` del servidor (semántica compatible con git). Cuando el remoto apunta a un noa-server:

```bash
noa remote add origin http://noa.example.com:3016/git/noa
noa push origin master
```

El push va por receive-pack y requiere una identidad autenticada (el token hace de contraseña; el rol debe permitir escribir).

## verify y el registro

`noa verify` es el escritor CLI del registro de verificación: los registros se añaden, con el schema `verify.run/1`, al log de agente del espacio de trabajo actual (JSONL, solo adición). Un registro puede llevar un enlace `hardware=<hw-id>` (junto con el audit por registro de hardware del lado del espacio de trabajo).

> El registro con autoridad está del lado de los archivos (`/mnt/work/verify-ledger/`); la escritura por CLI es una doble escritura fail-open — que el escritor abierto no verifique la cadena es una decisión de diseño; la verificación de la cadena corre a cargo del lector.
