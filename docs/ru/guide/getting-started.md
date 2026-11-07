# Начало работы

Эта страница проведёт вас от нуля до работающего noa: сборка, запуск сервера и выполнение первого
clone / push.

## Предварительные требования

- Rust stable ≥ 1.99 (устанавливается через `rustup`)
- git ≥ 2.39 (смарт-HTTP сервера вызывает `upload-pack` / `receive-pack` напрямую)
- Опционально: node + pnpm (для сборки WebUI)

## Сборка

```bash
git clone https://github.com/celestia-island/noa.git
cd noa
cargo build --release --locked
```

Артефакты: `target/release/noa` (CLI) и `target/release/noa-server` (сервер).

## Запуск сервера

Минимальной конфигурации нужен только один API-токен (стартовые учётные данные администратора):

```bash
export NOA_API_TOKEN="<your-token>"
export NOA_MIRROR_SCRATCH_DIR=/var/lib/noa/mirror-cache   # mirror scratch root
export NOA_MIRROR_ENABLED=1                               # enable mirror scheduling (default 900s)
export NOA_MIRROR_GH_TOKEN="<github-pat>"                 # optional: back-flow / canary push credential
noa-server --host 0.0.0.0 --port 3016
```

После запуска:

```bash
curl -H "Authorization: Bearer <your-token>" http://127.0.0.1:3016/api/v1/repos
```

Ответ `[]` означает, что сервис готов. Все REST-эндпоинты требуют bearer-аутентификации; то же
относится к путям чтения под `/git/{repo}/*` (неаутентифицированные запросы получают 401 с
challenge `Basic`).

## Нативный рабочий процесс noa (рекомендуется)

CLI `noa` — вход первого класса: клон — это репозиторий, где `.git` и `.noa` сосуществуют (git
управляет исходным кодом, noa — данными агентов):

```bash
noa clone http://<token>@127.0.0.1:3016/git/hello --path hello
cd hello
noa workspace create agent-a     # a per-agent isolated workspace
# … edit files …
noa snapshot create -m "change" -a agent-a
noa log                          # this workspace's snapshot history
noa push                         # publishes the workspace snapshot (serialized to a git commit)
```

`noa push` публикует **состояние снимка рабочего пространства** (создаётся коммит `[noa export]`),
а не коммиты из `.git`. Оба канала параллельны; не смешивайте их (сначала выполните `noa snapshot
create`, затем push — иначе опубликуете устаревший снимок). Аутентификация подставляется из
переменных окружения `NOA_GIT_USER` + `NOA_GIT_PASS`. Весь каталог (включая `.git/noa`)
копируется на другую машину и продолжает работать.

## Канал совместимости с git (подойдёт любой git-инструмент)

Тот же эндпоинт `/git/{repo}` говорит и на стандартном смарт-HTTP git — обычный git, CI и farm
работают без изменений:

```bash
git clone http://<token>@127.0.0.1:3016/git/hello
```

Push (receive-pack) требует роли с записью; приземлившийся push **немедленно запускает волну
импорта** (обратный поток продолжается в той же волне). Это переходная форма, которая оставляет
farm/CNB без изменений.

## Первое зеркало (загрузка репозитория GitHub в noa)

```bash
curl -X POST -H "Authorization: Bearer <your-token>" \
  -H "Content-Type: application/json" \
  -d '{
    "name": "hello",
    "remote_url": "https://github.com/you/hello.git",
    "github_slug": "you/hello",
    "branches": ["master"],
    "tags": true,
    "pr_heads": true,
    "enabled": true
  }' \
  http://127.0.0.1:3016/api/v1/mirror/jobs

curl -X POST -H "Authorization: Bearer <your-token>" \
  -H "Content-Type: application/json" \
  -d '{"job":"hello"}' \
  http://127.0.0.1:3016/api/v1/mirror/run
```

Когда «волна» завершится, репозиторий можно клонировать через noa:

```bash
git clone http://<your-token>@127.0.0.1:3016/git/hello
```

## Резервное копирование и миграция

Всё состояние сервера выгружается в один переносимый каталог (см. раздел резервного копирования
в [Развёртывании](../reference/deploy.md)): консистентный снимок redb + один клонируемый
git-bundle на репозиторий + манифест sha256; `noa-server --import-state <dir>` восстанавливает
офлайн. Критерий приёмки — учение: выгрузка → восстановление на другой машине → запуск → сверка.


## Дальнейшие шаги

- [Обзор архитектуры](./architecture.md) — модель рабочего пространства / снимков / ссылок (ref)
- [Зеркалирование и обратный поток](./mirror.md) — два канала, PR-головки, обратный поток и canary
- [Справочник HTTP API](../reference/http-api.md) — все эндпоинты
