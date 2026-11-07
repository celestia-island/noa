<p align="center"><img src="https://raw.githubusercontent.com/celestia-island/docs.celestia.world/dev/res/logo/noa.webp" alt="noa" width="200" /></p>

<h1 align="center">Noa</h1>

<p align="center"><strong>Распределённая система контроля версий, нативная для агентов</strong></p>

<div align="center">

[![License](https://img.shields.io/badge/license-SySL%201.0-blue)](../../LICENSE)
[![GitHub](https://img.shields.io/badge/github-celestia--island%2Fnoa-blue.svg)](https://github.com/celestia-island/noa)

</div>

<div align="center">

[English](../en/intro.md) ·
[简体中文](../zh-Hans/intro.md) ·
[繁體中文](../zh-Hant/intro.md) ·
[日本語](../ja/intro.md) ·
[한국어](../ko/intro.md) ·
[Français](../fr/intro.md) ·
[Español](../es/intro.md) ·
**Русский** ·
[العربية](../ar/intro.md)

</div>

Изоляция рабочего пространства для каждого агента, снимки с SHA-256-адресацией по содержимому,
полноценный сервер смарт-протокола git (clone/fetch/push), собственный forge со встроенным
зеркалированием и обратным потоком, построчное трёхстороннее слияние с точным определением
переименований, многопользовательский RBAC и API записей запусков CI — и всё это в едином
бинарном файле со встроенным хранилищем.

## Документация

Полная документация на английском языке находится в этом репозитории в `docs/en/` (этот сайт
зеркалирует её на [noa.docs.celestia.world](https://noa.docs.celestia.world)):

| Документация | Содержание |
|---|---|
| [Начало работы](./guide/getting-started.md) | Сборка, запуск сервера, первый clone/push |
| [Обзор архитектуры](./guide/architecture.md) | Модель рабочего пространства / снимков / ссылок (ref), компоненты сервера |
| [Зеркалирование и обратный поток](./guide/mirror.md) | Два канала, PR-головки, обратный поток и canary, проверка соответствия |
| [Права доступа и пользователи](./guide/rbac.md) | Роли, токены, применение правил в каналах REST и git |
| [Протокол git-http](./guide/git-http.md) | Эндпоинты смарт-протокола, семантика push, согласование протокола |
| [Слияние и конфликты](./guide/merge.md) | Построчный diff3, точное определение переименований, семантика деградации |
| [Записи запусков CI](./guide/ci-runs.md) | API запусков, интеграция с chest |
| [Справочник HTTP API](./reference/http-api.md) | Все REST-эндпоинты |
| [Справочник CLI](./reference/cli.md) | Полный набор команд командной строки |
| [Развёртывание](./reference/deploy.md) | systemd, переменные окружения, три обязательных шага развёртывания |

Первоисточник на упрощённом китайском находится в `docs/zh-Hans/`; остальные языки переводятся
с него пакетами.

Исходный код: [noa](https://github.com/celestia-island/noa).
