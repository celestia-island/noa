# Despliegue

Forma de despliegue de noa-server: binario único + base redb única + unidad de systemd. Las instancias de producción las supervisa malkuth.

## El trío de despliegue

1. **Artefactos**: el producto de `cargo build --release --locked --bin noa-server` se copia a la ruta de despliegue
   (reemplazo atómico: primero se escribe `<bin>.new` y luego se renombra), con verificación sha256 en ambos lados;
2. **Reintentos de health**: tras el arranque se sondea `/api/v1/*` (401 = proceso vivo y autenticación funcionando);
3. **Rollback**: se conserva la ruta del artefacto de la versión anterior, `mv` de vuelta + reinicio.

## Variables de entorno (configuración de arranque clase A → archivo env 0600)

| Variable | Descripción |
|---|---|
| `NOA_API_TOKEN` | token admin de arranque; **vacío / solo espacios en blanco = rechazo del arranque** |
| `NOA_MIRROR_ENABLED` | interruptor de la planificación del espejo |
| `NOA_MIRROR_INTERVAL_SECS` | intervalo entre olas (900 por defecto) |
| `NOA_MIRROR_SCRATCH_DIR` | raíz del scratch (p. ej. `/var/lib/noa/mirror-cache`) |
| `NOA_MIRROR_GH_TOKEN` | credenciales de escritura de GitHub para el reflujo / canary (nombre por defecto de token_env) |
| `NOA_MIRROR_GH_TOKEN_ENV` | nombre de la variable de entorno que sustituye a las credenciales (por defecto, la anterior) |

Las credenciales solo van al plano de despliegue (env 0600 / systemd LoadCredential), jamás a la base de datos, al árbol de git ni a la documentación.

## Unidad de systemd

```ini
[Service]
EnvironmentFile=/etc/noa/noa-server.env
ExecStart=/usr/local/bin/noa-server --host 0.0.0.0 --port 3016
Restart=on-failure
```

- Escucha en `0.0.0.0` (convención de los nodos de la familia: bearer en todas las rutas + aplicación del rol de escritura, superficie expuesta segura);
- Tras cambiar el env: `systemctl daemon-reload && systemctl restart`; verificar que
  `tr '\0' '\n' < /proc/$MAINPID/environ` contiene las claves nuevas;
- Las instancias de producción las supervisa malkuth (pasar por la exclusividad de claim antes de tocar la ruta de despliegue).

## Puertos y dominios

La asignación de puertos está en el registro del workspace (`_ledger/ports-and-domains.md`); la instancia de producción va en el 3016 y nginx, de puerta frontal, termina ahí el TLS.

## Instancias múltiples

redb es una base de archivo único — una instancia, un archivo de base. Varias instancias = varios directorios y varios puertos (como el modo de doble instancia de `chest-demo` y el chest de producción); el transporte de configuración entre instancias va por RPC import / export, sin copiar archivos de configuración.

## Copia de seguridad y restauración

Todo el estado del servidor = el archivo redb + el directorio scratch + el env (las
credenciales quedan **excluidas deliberadamente** — restaurar credenciales es
reprovisionar, nunca copiar).

```bash
# export (admin; live is fine — the write lock makes the copy consistent)
curl -X POST -H "Authorization: Bearer $ADMIN" -H "Content-Type: application/json" \
  -d '{"out": "/srv/noa-backup/nightly"}' \
  http://127.0.0.1:3016/api/v1/admin/export

# artifacts: noa.redb (consistent snapshot) + {job}.bundle (cloneable per-repo
# file) + manifest.json (sha256)

# restore (offline — restore before the database ever opens)
noa-server --db-path /new/noa.redb --import-state /srv/noa-backup/nightly --port 3016
```

La restauración verifica primero el sha256 de todos los artefactos (una copia a medio
corromper no escribe ni un archivo); un scratch existente se conserva (puede ir por
delante — la siguiente ola concilia). **El criterio de aceptación es el simulacro**:
exportar → restaurar en otra máquina → servir → conciliar. Se recomienda un timer de
systemd para exportaciones periódicas.

## Verificación tras el despliegue

```bash
# Auth and readiness
curl -s -o /dev/null -w '%{http_code}\n' http://127.0.0.1:3016/api/v1/prs   # expect 401
# Mirror wave
curl -X POST -H "Authorization: Bearer $T" -H "Content-Type: application/json" \
  -d '{"job":"noa"}' http://127.0.0.1:3016/api/v1/mirror/run               # expect 202
```

Detalles del espejo y semántica del reflujo: ver [Espejo y reflujo](../guide/mirror.md).
