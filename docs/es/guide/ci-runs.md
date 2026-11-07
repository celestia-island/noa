# Registros de ejecuciones de CI (API de runs)

noa incorpora un almacenamiento de registros de ejecuciones de CI: un «flujo de comprobaciones ejecutado sobre el head de un repositorio» constituye una run. Es la fuente de datos de la vía `CI_BACKEND=noa` de la vista de CI del panel DevOps de chest.

## Endpoints

| Método | Ruta | Descripción |
|---|---|---|
| POST | `/api/v1/repos/{name}/runs` | registra una run (rol de escritura) |
| GET | `/api/v1/repos/{name}/runs?limit=&offset=` | lista, las más recientes primero |

- `{name}` debe existir en el catalog (si no, 404);
- `number` lo asigna el servidor por repositorio (atómico: el barrido y la escritura van en la misma transacción de escritura; registros concurrentes no se pisan el número);
- validación: `head_sha` 40 hex; `status` ∈ `queued | running | success | failure | cancelled`;
  `kind` 1..=64 caracteres; `actor` 1..=200; `log_ref` / `summary` ≤ 2000.

## Registrar una run

```bash
curl -X POST -H "Authorization: Bearer $TOKEN" -H "Content-Type: application/json" \
  -d '{
    "head_sha": "<40-hex>",
    "kind": "basic-check",
    "status": "success",
    "actor": "farm-runner-1",
    "log_ref": "https://ci.example.com/logs/123"
  }' \
  http://127.0.0.1:3016/api/v1/repos/noa/runs
```

Respuesta (201): el servidor completa `repo`, `number`, `started_us` (cuando el cliente no los proporciona).

## Consulta

```bash
curl -H "Authorization: Bearer $TOKEN" \
  "http://127.0.0.1:3016/api/v1/repos/noa/runs?limit=10&offset=0"
```

Las filas devueltas tienen esta forma:

```json
{
  "repo": "noa",
  "number": 1,
  "head_sha": "…",
  "kind": "basic-check",
  "status": "success",
  "actor": "farm-runner-1",
  "started_us": 1794049465562618
}
```

## Límites de diseño

- **Los logs no se almacenan en noa**: el registro lleva un puntero opaco `log_ref` que apunta al almacenamiento de logs del pipeline de orquestación;
- **El contenido de las filas tiene un tope** (ver validación) — para evitar que un escritor autenticado hinche el volumen de las filas;
- **Almacenamiento**: tabla redb `runs`, clave `{repo}#{number:020}` (orden lexicográfico = orden temporal dentro del repo).

## Integración con chest

El manejador `ci_ops` de chest lee la variable de entorno `CI_BACKEND`:

- `github` (por defecto): vía v1 de GitHub Actions, comportamiento idéntico al de las versiones históricas;
- `noa`: lee la API de runs de noa apuntada por `NOA_API_BASE` + `NOA_API_TOKEN` y normaliza las filas a la misma forma de panel que las filas de GitHub (`kind`→name, `number`→run_number, `started_us`→RFC3339
  `created_at`, estados finales mapeados a conclusion); la lista de watch usa `CI_NOA_REPOS` (nombres de directorio de un solo segmento) y, si no está definida, cae al último segmento de `CI_GITHUB_REPOS`.

El `ci.config.get` del panel informa el campo `backend`; `configured` depende del backend — un despliegue pure-noa (sin token de GitHub) se considera conectado en cuanto el trío noa está completo.
