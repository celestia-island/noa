# Развёртывание

Форма развёртывания noa-server: один бинарник + одна база redb + systemd-юнит. Продакшн-инстансы
курируются malkuth.

## Три обязательных шага развёртывания

1. **Артефакт**: результат `cargo build --release --locked --bin noa-server` копируется на путь
   развёртывания (атомарная замена: сначала пишется `<bin>.new`, затем rename), sha256
   проверяется с обеих сторон;
2. **Проверка живости с повторами**: после запуска опрашивается `/api/v1/*` (401 = процесс жив и
   аутентификация работает);
3. **Откат**: сохраняется путь предыдущего артефакта; `mv` обратно + рестарт.

## Переменные окружения (конфиг класса A для загрузки → env-файл 0600)

| Переменная | Описание |
|---|---|
| `NOA_API_TOKEN` | стартовый токен администратора; **пустое / из одних пробелов — отказ запуска** |
| `NOA_MIRROR_ENABLED` | выключатель планировщика зеркалирования |
| `NOA_MIRROR_INTERVAL_SECS` | интервал волны (по умолчанию 900) |
| `NOA_MIRROR_SCRATCH_DIR` | корень scratch (например `/var/lib/noa/mirror-cache`) |
| `NOA_MIRROR_GH_TOKEN` | учётные данные на запись GitHub для обратного потока / canary (имя `token_env` по умолчанию) |
| `NOA_MIRROR_GH_TOKEN_ENV` | переопределяет имя переменной окружения с учётными данными (по умолчанию — предыдущая) |

Учётные данные попадают только на поверхность развёртывания (env 0600 / systemd LoadCredential),
никогда — в БД, git-дерево или документацию.

## systemd-юнит

```ini
[Service]
EnvironmentFile=/etc/noa/noa-server.env
ExecStart=/usr/local/bin/noa-server --host 0.0.0.0 --port 3016
Restart=on-failure
```

- Слушает `0.0.0.0` (конвенция family-узлов: bearer-аутентификация на всех маршрутах + применение
  роли на запись — безопасная поверхность экспозиции);
- После изменения env: `systemctl daemon-reload && systemctl restart`; проверьте, что
  `tr '\0' '\n' < /proc/$MAINPID/environ` содержит новые ключи;
- Продакшн-инстансы курируются malkuth (перед работой с путями развёртывания возьмите claim на
  эксклюзивность развёртывания).

## Порты и домены

Выделение портов живёт в реестре рабочего пространства (`_ledger/ports-and-domains.md`);
продакшн-инстанс — 3016, поверх — nginx-вход с терминированием TLS.

## Несколько инстансов

redb — однофайловое хранилище: один инстанс — один файл базы. Несколько инстансов = несколько
каталогов, несколько портов (например, двухинстансный режим `chest-demo` + продакшн-chest);
перенос конфигурации между инстансами идёт через RPC import / export, никогда — копированием
конфиг-файлов.

## Резервное копирование и восстановление

Всё состояние сервера = файл redb + scratch-каталог + env (учётные данные **намеренно исключены** —
восстановление учётных данных это повторное размещение, а не копирование).

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

Восстановление сначала проверяет sha256 каждого артефакта (наполовину повреждённая резервная
копия не записывает ни одного файла); существующий scratch сохраняется (он может опережать —
следующая волна сверит). **Критерий приёмки — учение**: выгрузка → восстановление на другой
машине → запуск → сверка. Рекомендуется systemd-timer для регулярных выгрузок.

## Проверка после развёртывания

```bash
# Auth and readiness
curl -s -o /dev/null -w '%{http_code}\n' http://127.0.0.1:3016/api/v1/prs   # expect 401
# Mirror wave
curl -X POST -H "Authorization: Bearer $T" -H "Content-Type: application/json" \
  -d '{"job":"noa"}' http://127.0.0.1:3016/api/v1/mirror/run               # expect 202
```

Детали зеркалирования и семантика обратного потока: см.
[Зеркалирование и обратный поток](../guide/mirror.md).
