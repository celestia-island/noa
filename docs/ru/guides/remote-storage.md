# Руководство по удалённому хранилищу

## Обзор

noa поддерживает несколько серверных хранилищ для распространения и резервного
копирования объектов, адресованных по содержимому. Хранилища настраиваются для
каждого репозитория и управляются через единую команду `noa storage`.

## Поддерживаемые хранилища

| Хранилище | Тип | Требуется | Сценарий использования |
|-----------|-----|-----------|------------------------|
| IPFS (Kubo) | `ipfs` | Запущенный демон IPFS | Децентрализованное P2P-распространение |
| S3 / MinIO | `s3` | S3-совместимая конечная точка | Централизованное резервное копирование, облачное хранилище |

## Добавление хранилища

### IPFS

Сначала запустите демон Kubo:

```bash
ipfs daemon &   # слушает на 127.0.0.1:5001
```

Добавьте хранилище:

```bash
# Добавить IPFS со значениями по умолчанию (endpoint=http://127.0.0.1:5001, gateway=https://ipfs.io)
noa storage add ipfs-local --type ipfs

# Настроить конечную точку и шлюз
noa storage add ipfs-local --type ipfs \
  --endpoint http://192.168.1.100:5001 \
  --gateway https://dweb.link

# Использовать удалённый сервис закрепления (например, Pinata)
noa storage add pinata --type ipfs \
  --endpoint https://api.pinata.cloud/psa \
  --auth-token YOUR_PINATA_TOKEN

# Включить автоматическое закрепление при каждой отправке
noa storage add ipfs-local --type ipfs --auto-pin
```

### S3 / MinIO

```bash
# Добавить S3-совместимое хранилище
noa storage add s3-backup --type s3 \
  --endpoint https://s3.us-east-1.amazonaws.com \
  --bucket my-noa-objects \
  --access-key AKIAIOSFODNN7EXAMPLE \
  --secret-key wJalrXUtnFEMI/K7MDENG/bPxRfiCYEXAMPLEKEY \
  --region us-east-1

# Добавить локальный сервер MinIO
noa storage add minio-local --type s3 \
  --endpoint http://localhost:9000 \
  --bucket noa \
  --access-key minioadmin \
  --secret-key minioadmin
```

## Управление хранилищами

```bash
# Список всех настроенных хранилищ
noa storage list
# ipfs-local    http://127.0.0.1:5001 (ipfs) [gateway=https://ipfs.io]
# s3-backup     https://s3.example.com (s3) [bucket=noa-objects]

# Проверить состояние соединения
noa storage status               # все хранилища
noa storage status ipfs-local    # конкретное хранилище

# Удалить хранилище
noa storage remove s3-backup
```

## Отправка снимков

Отправляйте объекты в удалённое хранилище для распространения или резервного
копирования:

```bash
# Отправить все снимки в конкретное хранилище
noa storage push --target ipfs-local

# Отправить и закрепить (только IPFS — предотвращает сборку мусора)
noa storage push --target ipfs-local --pin

# Отправить конкретный снимок
noa storage push --target s3-backup --snapshot noa_abc123

# Отправить все снимки из рабочего пространства
noa storage push --target ipfs-local --workspace feature-auth --pin
```

С `auto_pin = true` в конфигурации `--pin` подразумевается. Вы также можете
отправить сразу во все хранилища с автозакреплением, опустив `--target`:

```bash
noa storage push --pin   # отправляет во все хранилища с auto_pin=true
```

## Получение объектов

Скачайте объект из удалённого хранилища и сохраните его локально:

```bash
# Получить по SHA-256-хешу (любое хранилище)
noa storage fetch ipfs-local 2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824

# Получить по CID (только IPFS)
noa storage fetch ipfs-local bafkreihdwdcefgh4dqkjv67uzcmw7ojel6viejvs6buyq7omaygs5sep7m
```

## Как работает отправка (push)

1. **Сначала локально**: noa читает объекты из локального `RedbObjectStore`
2. **Рекурсивная передача**: Для каждого снимка обходится всё дерево (блобы и
   поддеревья). Объекты, отсутствующие на удалённом сервере, передаются.
3. **Адресация по содержимому**: Оба хранилища используют SHA-256. Для IPFS хеши
   преобразуются в CIDv1 (кодек raw). Для S3 хеши используются как ключи
   объектов.
4. **Закрепление** (только IPFS): После отправки `--pin` указывает демону
   сохранять объекты, предотвращая сборку мусора.

## Формат конфигурации

```toml
# .noa/config

[[storage]]
name = "ipfs-local"
type = "ipfs"
endpoint = "http://127.0.0.1:5001"
gateway = "https://ipfs.io"
auto_pin = true

[[storage]]
name = "s3-backup"
type = "s3"
endpoint = "https://s3.example.com"
bucket = "noa-objects"
access_key = "AKIA..."
secret_key = "..."
region = "us-east-1"
```

## Программное использование

```rust
use libnoa::config::StorageConfig;
use libnoa::object::{create_remote_store, ObjectStore};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cfg = StorageConfig::ipfs("local", "http://127.0.0.1:5001");
    let store = create_remote_store(&cfg).await?;

    // Сохранить содержимое удалённо
    let blob_id = store.put_blob(b"hello world").await?;
    println!("Stored as: {}", blob_id);

    // Проверить наличие
    assert!(store.has_blob(&blob_id).await?);

    // Получить
    let data = store.get_blob(&blob_id).await?;
    assert_eq!(data, b"hello world");

    Ok(())
}
```
