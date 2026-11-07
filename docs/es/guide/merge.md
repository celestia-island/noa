# Fusión y conflictos

La fusión de noa es una estructura de dos niveles «recursión por árbol + hojas por línea»: los directorios se recorren recursivamente nivel a nivel y un conflicto a nivel de archivo intenta primero una fusión de tres vías a nivel de línea (diff3), y si falla cae en un conflicto estructurado.

## Fusión de tres vías a nivel de línea (W2④)

Ante modificaciones de ambas partes en un mismo archivo, primero se hace un barrido diff3 tomando el ancestro común como base:

- **Zonas tocadas por una sola parte**: se toma esa parte directamente — las ediciones no superpuestas se fusionan automáticamente y ya no producen un conflicto de todo el blob;
- **Zonas superpuestas o adyacentes** (igual que git, incluso sin hueco entre ellas cuenta como conflicto): se genera un bloque de marcadores de conflicto al estilo git
  (`<<<<<<< ours` / `=======` / `>>>>>>> theirs`), conservando a la vez el conflicto estructurado para resolverlo en el panel;
- **Contenido idéntico o una parte sin cambios**: camino rápido que toma directamente la parte disponible.

```rust
use libnoa::merge::line_merge::{merge_lines, LineMerge};

match merge_lines(base, ours, theirs) {
    LineMerge::Clean(merged) => { /* 自动解决 */ }
    LineMerge::Conflict { merged, ours, theirs, base } => { /* 结构化冲突 */ }
}
```

## Renombrado por contenido exacto (W2④, segunda pasada)

Cuando una parte hace un «renombrado puro» (borra la ruta antigua y el contenido del blob reaparece intacto en la ruta nueva) y la otra parte **modifica la ruta antigua**, la modificación se aplica automáticamente en la **ruta nueva** — el escenario clásico rename+edit ya no produce un conflicto de «borrado vs modificación».

Casos no declarados (caen de forma conservadora en conflicto estructurado):

- renombrado que además cambia el contenido (rename con carga);
- ambas partes renombran a destinos distintos;
- la ruta destino es tocada por la otra parte (colisión);
- rename vs delete (el rename transporta el contenido base y sobrevive en la ruta nueva — semántica rename-aware de git).

La heurística de similitud (reconocimiento de renombrados sin coincidencia exacta) es un corte posterior.

## Semántica de degradación

Cuando la fusión a nivel de línea no está disponible (blob ausente/dañado/binario/con un CR suelto), el archivo cae de vuelta en un conflicto estructurado de todo el blob
— nunca se descarta el archivo en silencio y nunca se devuelve un 500. La forma de fixture con las tres partes vacías (el almacenamiento materializa los id ausentes como blobs vacíos) también se degrada.

## Flujo de fusión de PRs

`POST /api/v1/repos/{name}/prs/{number}/merge`:

1. fusión de tres vías de instantáneas (base = base de la PR, ours = espacio de trabajo destino, theirs = head de la PR);
2. sin conflictos → se escribe el árbol automáticamente + se avanzan las referencias;
3. con conflictos → 409 + lista de conflictos (ruta + ids de blob ours/theirs/base), que el panel resuelve con
   `ConflictResolution::{Ours, Theirs}`.

## Compatibilidad con git

El resultado de la fusión satisface el álgebra de objetos de git: el árbol resuelto se puede reescribir directamente en el scratch y refluir (push de reflujo + verificación de fidelidad, sin conversión alguna).
