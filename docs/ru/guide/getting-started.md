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

## Первый push (полномочия записи)

Push идёт через путь receive-pack под `/git/{repo}` и требует аутентифицированной идентичности:

```bash
cd hello
echo "hello noa" > README.md
git add -A && git commit -m "hello noa"
git push origin master
```

Push попадает в scratch-каталог зеркала; следующая волна зеркалирования автоматически загружает
его в систему снимков/ссылок (snapshot/ref) noa, а если у задачи включён `backflow`, состояние
также отправляется обратно в origin (GitHub) — переходная форма схемы «noa как источник истины +
зеркало GitHub».

## Дальнейшие шаги

- [Обзор архитектуры](./architecture.md) — модель рабочего пространства / снимков / ссылок (ref)
- [Зеркалирование и обратный поток](./mirror.md) — два канала, PR-головки, обратный поток и canary
- [Справочник HTTP API](../reference/http-api.md) — все эндпоинты
