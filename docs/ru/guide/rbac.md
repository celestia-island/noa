# Права доступа и пользователи

Аутентификация в noa — «один токен, два канала»: REST и git используют одни и те же учётные
данные, но право на запись применяется на каждом канале отдельно.

## Учётные данные

- **Стартовый токен**: переменная окружения `NOA_API_TOKEN`. Сервер отказывается стартовать с
  пустым значением (пустое / состоящее из одних пробелов отклоняется при запуске); этот токен —
  идентичность администратора.
- **Токены зарегистрированных пользователей**: создаются через REST (см. ниже), хранятся в виде
  sha256-хэшей, сравниваются за постоянное время.

Формы аутентификации (одинаковы на обоих каналах):

```text
Authorization: Bearer <token>
Authorization: Basic <base64(any-user:token)>     # git client convention: any username
```

Сервер с пустым токеном (`NOA_API_TOKEN=""`) не деградирует до анонимного чтения — запуск
отклоняется сразу, а проверка на непустоту перед сравнением добавляет эшелонированную защиту.

## Роли

| Роль | Чтение | Запись (REST / git push) | Админ (CRUD пользователей) |
|---|---|---|---|
| admin | ✓ | ✓ | ✓ |
| operator | ✓ | ✓ | ✗ |
| reader | ✓ | ✗ | ✗ |

- REST: применение правил сосредоточено в посреднике (middleware) — запросы на запись от reader
  получают 403; не-GET на `/api/v1/users` получает 403 (только admin).
- git: push (receive-pack) требует can_write — без учётных данных 401 (с challenge `Basic`,
  который git-клиенты используют для запроса пароля); учётные данные есть, но роль reader — 403
  (сообщение о том, что нужна роль на запись). clone/fetch работают с любой аутентифицированной
  идентичностью.

## REST управления пользователями

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

Правила:

- `name` и `role` (admin/operator/reader) обязательны; сервер хэширует токен через sha256 перед
  сохранением.
- Обновление (POST на тот же эндпоинт с тем же именем) может сменить токен или изменить роль;
  `created_us` сохраняет исходное значение.
- Удаление доступно только admin и необратимо.

## Пример git-канала

```bash
# Read (any authenticated identity; public repos work with a placeholder username)
git clone http://<token>@noa.example.com:3016/git/noa

# Write (requires operator/admin)
git push origin master
```

Неаутентифицированный push получает 401 + challenge `Basic` (git-клиенты используют его для
запроса учётных данных); push от идентичности с ролью reader получает 403 (сообщение о том, что
требуется роль на запись).
