# Permisos y usuarios

La autenticación de noa sigue el principio «un solo token, dos canales»: REST y git comparten las mismas credenciales, pero los permisos de escritura se aplican por separado en cada canal.

## Credenciales

- **Token de arranque**: la variable de entorno `NOA_API_TOKEN`. El servidor se niega a arrancar si está vacía (vacía o solo espacios en blanco = rechazo del arranque); ese token es la identidad admin.
- **Tokens de usuarios registrados**: se crean por REST (ver más abajo), se almacenan con hash sha256 y se comparan en tiempo constante.

Formas de autenticación (idénticas en ambos canales):

```text
Authorization: Bearer <token>
Authorization: Basic <base64(any-user:token)>     # git client convention: any username
```

Un servidor con token vacío (`NOA_API_TOKEN=""`) no degrada a lectura anónima — el arranque se rechaza de inmediato, y la guarda de no-vacío antes de comparar añade una doble defensa.

## Roles

| Rol | Lectura | Escritura (REST / git push) | Administración (CRUD de usuarios) |
|---|---|---|---|
| admin | ✓ | ✓ | ✓ |
| operator | ✓ | ✓ | ✗ |
| reader | ✓ | ✗ | ✗ |

- REST: el middleware aplica de forma centralizada — las peticiones de escritura de reader reciben 403; `/api/v1/users` fuera de GET recibe 403 (solo admin).
- git: el push (receive-pack) exige can_write — sin credenciales 401 (con challenge Basic, con lo que el cliente git pide credenciales); con credenciales pero rol reader, 403 (indicando que se necesita el rol write). El clone/fetch está abierto a cualquier identidad autenticada.

## REST de gestión de usuarios

```bash
# Create (admin)
curl -X POST -H "Authorization: Bearer $ADMIN" -H "Content-Type: application/json" \
  -d '{"name":"alice","role":"operator","token":"<alice-token>"}' \
  http://127.0.0.1:3016/api/v1/users

# List (admin) / single
curl -H "Authorization: Bearer $ADMIN" http://127.0.0.1:3016/api/v1/users
curl -H "Authorization: Bearer $ADMIN" http://127.0.0.1:3016/api/v1/users/alice

# Delete (admin)
curl -X DELETE -H "Authorization: Bearer $ADMIN" \
  http://127.0.0.1:3016/api/v1/users/alice
```

Reglas:

- `name` y `role` (admin/operator/reader) son obligatorios; el servidor aplica sha256 al token antes de almacenarlo.
- La actualización (POST del mismo nombre en el mismo endpoint) permite rotar el token o cambiar el rol; `created_us` conserva su valor original.
- El borrado es exclusivo del admin e irreversible.

## Ejemplo del canal git

```bash
# Read (any authenticated identity; public repos work with a placeholder username)
git clone http://<token>@noa.example.com:3016/git/noa

# Write (requires operator/admin)
git push origin master
```

Un push no autenticado recibe 401 + challenge Basic (el cliente git pide credenciales a partir de ahí); un push con identidad reader recibe
403 (indicando que se necesita el rol write).
