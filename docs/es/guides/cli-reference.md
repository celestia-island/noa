# Referencia de CLI

## `noa init [path]`

Inicializa un nuevo repositorio `.noa/`. Crea `noa.redb`, `agent-logs/`, `HEAD` y `config`.

```bash
noa init .           # directorio actual
noa init /path/repo  # ruta específica
```

## `noa status`

Muestra el espacio de trabajo actual y la instantánea cabeza.

```bash
noa status
# En el espacio de trabajo: default (cabeza: noa_abc123, mensaje: initial)
```

## `noa log [options]`

Ver el historial de instantáneas.

| Bandera | Valor por defecto | Descripción |
|------|---------|-------------|
| `-w, --workspace` | HEAD actual | Filtrar por espacio de trabajo |
| `-l, --limit` | 20 | Máximo de entradas a mostrar |

```bash
noa log
noa log --workspace feature-1 --limit 50
```

## `noa snapshot <subcommand>`

### `noa snapshot create [-m msg] [-a author]`

Crear una instantánea desde el registro del agente del espacio de trabajo actual.

```bash
noa snapshot create -m "añadir funcionalidad de login" -a "agent-001"
```

### `noa snapshot list`

Listar todas las instantáneas en todos los espacios de trabajo.

### `noa snapshot diff <a> <b>`

Mostrar diferencias a nivel de archivo entre dos instantáneas.

```bash
noa snapshot diff noa_abc123 noa_def456
```

## `noa workspace <subcommand>`

### `noa workspace create <name> [--agent <id>]`

Crear un nuevo espacio de trabajo bifurcado desde el HEAD actual.

### `noa workspace switch <name>`

Cambiar el espacio de trabajo activo (actualiza HEAD).

### `noa workspace list`

Listar todos los espacios de trabajo. `*` marca el activo.

### `noa workspace delete <name>`

Eliminar un espacio de trabajo (no se puede eliminar el espacio de trabajo activo).

### `noa workspace merge <from>`

Fusionar otro espacio de trabajo en el actual usando fusión a tres vías.

```bash
noa workspace switch default
noa workspace merge feature-1
```

## `noa remote <subcommand>`

### `noa remote add <name> <url>`

Añadir un repositorio remoto.

### `noa remote remove <name>`

Eliminar un remoto.

### `noa remote list`

Listar todos los remotos configurados.

## `noa push [--remote name]`

Hacer push a un remoto (aún no implementado).

## `noa pull [--remote name]`

Hacer pull desde un remoto (aún no implementado).

## `noa fetch [--remote name]`

Obtener de un remoto sin fusionar (aún no implementado).

## `noa clone <url> [path]`

Clonar un repositorio remoto (aún no implementado).
