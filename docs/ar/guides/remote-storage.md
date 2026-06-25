# دليل التخزين البعيد

## نظرة عامة

يدعم noa خوادم تخزين بعيدة متعددة لتوزيع ونسخ احتياطي للكائنات المعنونة
بالمحتوى. تُهيَّأ الخوادم لكل مستودع وتُدار عبر الأمر الموحد `noa storage`.

## الخوادم المدعومة

| الخادم | النوع | يتطلب | حالة الاستخدام |
|--------|-------|-------|----------------|
| IPFS (Kubo) | `ipfs` | خادم IPFS قيد التشغيل | توزيع P2P لامركزي |
| S3 / MinIO | `s3` | نقطة نهاية متوافقة مع S3 | نسخ احتياطي مركزي، تخزين سحابي |

## إضافة خادم تخزين

### IPFS

أولًا، ابدأ خادم Kubo:

```bash
ipfs daemon &   # يستمع على 127.0.0.1:5001
```

أضف الخادم:

```bash
# إضافة IPFS بالقيم الافتراضية (endpoint=http://127.0.0.1:5001, gateway=https://ipfs.io)
noa storage add ipfs-local --type ipfs

# تخصيص نقطة النهاية والبوابة
noa storage add ipfs-local --type ipfs \
  --endpoint http://192.168.1.100:5001 \
  --gateway https://dweb.link

# استخدام خدمة تثبيت بعيدة (مثل Pinata)
noa storage add pinata --type ipfs \
  --endpoint https://api.pinata.cloud/psa \
  --auth-token YOUR_PINATA_TOKEN

# تمكين التثبيت التلقائي عند كل دفعة
noa storage add ipfs-local --type ipfs --auto-pin
```

### S3 / MinIO

```bash
# إضافة خادم متوافق مع S3
noa storage add s3-backup --type s3 \
  --endpoint https://s3.us-east-1.amazonaws.com \
  --bucket my-noa-objects \
  --access-key AKIAIOSFODNN7EXAMPLE \
  --secret-key wJalrXUtnFEMI/K7MDENG/bPxRfiCYEXAMPLEKEY \
  --region us-east-1

# إضافة خادم MinIO محلي
noa storage add minio-local --type s3 \
  --endpoint http://localhost:9000 \
  --bucket noa \
  --access-key minioadmin \
  --secret-key minioadmin
```

## إدارة الخوادم

```bash
# سرد جميع الخوادم المُهيأة
noa storage list
# ipfs-local    http://127.0.0.1:5001 (ipfs) [gateway=https://ipfs.io]
# s3-backup     https://s3.example.com (s3) [bucket=noa-objects]

# التحقق من حالة الاتصال
noa storage status               # جميع الخوادم
noa storage status ipfs-local    # خادم محدد

# إزالة خادم
noa storage remove s3-backup
```

## دفع اللقطات

ادفع الكائنات إلى خادم بعيد للتوزيع أو النسخ الاحتياطي:

```bash
# دفع جميع اللقطات إلى خادم محدد
noa storage push --target ipfs-local

# الدفع والتثبيت (IPFS فقط — يمنع التجميع الآلي للقمامة)
noa storage push --target ipfs-local --pin

# دفع لقطة محددة
noa storage push --target s3-backup --snapshot noa_abc123

# دفع جميع اللقطات من مساحة عمل
noa storage push --target ipfs-local --workspace feature-auth --pin
```

مع `auto_pin = true` في التهيئة، يكون `--pin` ضمنيًا. يمكنك أيضًا الدفع إلى
جميع خوادم التثبيت التلقائي دفعة واحدة بحذف `--target`:

```bash
noa storage push --pin   # يدفع إلى جميع الخوادم ذات auto_pin=true
```

## جلب الكائنات

نزِّل كائنًا من خادم بعيد وخزّنه محليًا:

```bash
# الجلب عبر تجزئة SHA-256 (أي خادم)
noa storage fetch ipfs-local 2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824

# الجلب عبر CID (IPFS فقط)
noa storage fetch ipfs-local bafkreihdwdcefgh4dqkjv67uzcmw7ojel6viejvs6buyq7omaygs5sep7m
```

## كيف يعمل الدفع

1. **المحلي أولًا**: يقرأ noa الكائنات من `RedbObjectStore` المحلي
2. **النقل التكراري**: لكل لقطة، يُتنقَّل في الشجرة بأكملها (القطع والأشجار
   الفرعية). تُنقل الكائنات غير الموجودة على الخادم البعيد.
3. **العنونة بالمحتوى**: يستخدم كلا الخادمين SHA-256. بالنسبة لـ IPFS،
   تُحوَّل التجزئات إلى CIDv1 (ترميز raw). بالنسبة لـ S3، تُستخدم التجزئات
   كمفاتيح كائنات.
4. **التثبيت** (IPFS فقط): بعد الدفع، يُخبر `--pin` الخادم بالاحتفاظ
   بالكائنات، مما يمنع التجميع الآلي للقمامة.

## تنسيق التهيئة

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

## الاستخدام البرمجي

```rust
use libnoa::config::StorageConfig;
use libnoa::object::{create_remote_store, ObjectStore};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cfg = StorageConfig::ipfs("local", "http://127.0.0.1:5001");
    let store = create_remote_store(&cfg).await?;

    // تخزين المحتوى عن بُعد
    let blob_id = store.put_blob(b"hello world").await?;
    println!("Stored as: {}", blob_id);

    // التحقق من الوجود
    assert!(store.has_blob(&blob_id).await?);

    // الاسترجاع
    let data = store.get_blob(&blob_id).await?;
    assert_eq!(data, b"hello world");

    Ok(())
}
```
