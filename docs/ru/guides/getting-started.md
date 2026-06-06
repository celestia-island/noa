# Начало работы

## Необходимые компоненты

- Rust 1.75+ (stable)
- Python 3.8+ (для сборочных скриптов)
- `just` command runner

## Установка

```bash
git clone https://github.com/celestia-island/noa.git
cd noa
just init          # получение зависимостей
just build-dev     # сборочная сборка
```

Бинарный файл `noa` находится в `target/debug/noa`.

## Быстрый старт

```bash
# Инициализировать новый репозиторий
noa init .

# Проверить статус
noa status
# В рабочей области: default

# Создать рабочую область
noa workspace create feature-1

# Переключиться на неё
noa workspace switch feature-1

# Создать снимок
noa snapshot create -m "начальная работа"

# Просмотреть историю
noa log

# Переключиться обратно и слить
noa workspace switch default
noa workspace merge feature-1

# Управление удалёнными репозиториями
noa remote add origin https://github.com/example/repo.git
noa remote list
```

## Запуск примеров

```bash
python3 examples/run_all.py
```

## Разработка

```bash
just fmt            # форматирование кода
just clippy         # линтинг
just test           # запуск тестов
just check          # проверка типов
```
