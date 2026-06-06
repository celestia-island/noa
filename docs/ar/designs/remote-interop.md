# تصميم التوافق البُعدي

## نظرة عامة

يدعم noa خلفيات بعيدة متعددة لمزامنة اللقطات والكائنات عبر الأجهزة والفرق. الهدف الأساسي للتوافق هو Git، مما يتيح تكاملًا سلسًا مع سير العمل الحالي على GitHub و GitLab و Bitbucket.

## واجهة الخلفية البعيدة (RemoteBackend Trait)

```rust
#[async_trait]
pub trait RemoteBackend: Send + Sync {
    async fn push_snapshots(&self, ids: &[SnapshotId]) -> Result<()>;
    async fn fetch_snapshots(&self, ids: &[SnapshotId]) -> Result<Vec<Snapshot>>;
    async fn push_objects(&self, ids: &[String]) -> Result<()>;
    async fn fetch_objects(&self, ids: &[String]) -> Result<()>;
    async fn list_refs(&self) -> Result<HashMap<String, SnapshotId>>;
    async fn update_ref(&self, name: &str, old: Option<&SnapshotId>, new: &SnapshotId) -> Result<()>;
}
```

## طبقة الترجمة إلى Git

يحول `GitTranslator` بين نموذج كائنات noa ونموذج Git:

### Blob ↔ Git Blob

```mermaid
graph LR
    subgraph Noa
        NB["noa blob:<br/>بايتات خام<br/>تجزئة SHA-256"]
    end
    subgraph Git
        GB["Git blob:<br/>'blob &lt;size&gt;\\0&lt;content&gt;'<br/>تجزئة SHA-1"]
    end
    NB -- "إعادة تجزئة المحتوى مع<br/>تنسيق ترويسة blob في Git" --> GB
```

### Tree ↔ Git Tree

```mermaid
graph LR
    subgraph Noa
        NT["noa tree:<br/>MessagePack [{name, kind, hash}]"]
    end
    subgraph Git
        GT["Git tree:<br/>'&lt;mode&gt; &lt;name&gt;\\0&lt;20-byte-sha1&gt;' مدخلات"]
    end
    NT -- "TreeEntry::Blob → mode 100644<br/>TreeEntry::Tree → mode 040000<br/>إعادة تجزئة SHA-256 → SHA-1" --> GT
```

### Snapshot ↔ Git Commit

```mermaid
graph LR
    subgraph "noa Snapshot"
        NS["id: noa_abc123<br/>tree_hash: SHA-256<br/>parents: [noa_...]<br/>author: agent-001<br/>timestamp: 1717592400000000 (µs)<br/>message: 'add feature'"]
    end
    subgraph "Git Commit"
        GC["tree: SHA-1<br/>parent: SHA-1<br/>author: agent-001 &lt;agent@noa&gt;<br/>message: 'add feature'"]
    end
    NS -- "tree_hash معاد تجزئته (SHA-256 → SHA-1)<br/>الآباء معينون عبر بحث ID<br/>المؤلف منسق ببريد إلكتروني وهمي<br/>الطابع الزمني µs مقطوع إلى ثوانٍ" --> GC
```

### Workspace ↔ Git Branch

```mermaid
graph LR
    subgraph Noa
        NW["workspace 'feature-1'<br/>(head: noa_abc123)"]
        ND["workspace 'default'<br/>(head: noa_def456)"]
    end
    subgraph Git
        GB1["branch 'feature-1'<br/>(HEAD: git-sha1)"]
        GB2["branch 'main'<br/>(HEAD: git-sha1)"]
    end
    NW --> GB1
    ND --> GB2
```

### تعيين المراجع (Ref Mapping)

```mermaid
graph LR
    subgraph "noa Refs"
        NH["HEAD → default"]
        ND2["default → noa_abc"]
        NF["feature-1 → noa_def"]
    end
    subgraph "Git Refs"
        GH["HEAD → refs/heads/main"]
        GMAIN["refs/heads/main → git-sha1"]
        GF1["refs/heads/feature-1 → git-sha2"]
    end
    NH -.-> GH
    ND2 -.-> GMAIN
    NF -.-> GF1
```

## سير الدفع (Push)

```mermaid
flowchart TD
    A["1. noa push --remote origin"] --> B["2. تحميل جميع اللقطات الموصولة من رؤوس مساحات العمل"]
    B --> C["3. ترجمة كل لقطة → Git commit"]
    C --> D["4. ترجمة كل blob/tree → كائن Git"]
    D --> E["5. الدفع عبر gix (gitoxide) إلى عنوان URL الأصلي"]
    E --> F["6. تحديث المراجع البعيدة"]
```

## سير السحب (Pull)

```mermaid
flowchart TD
    A["1. noa pull --remote origin"] --> B["2. جلب المراجع عبر gix"]
    B --> C["3. لكل Git commit جديد:"]
    C --> D["أ. ترجمة إلى لقطة noa<br/>ب. ترجمة blobs/trees إلى كائنات noa<br/>ج. تخزين في redb المحلي"]
    D --> E["4. إنشاء لقطة دمج (رأس محلي + رأس بعيد)"]
    E --> F["5. تحديث رأس مساحة العمل"]
```

## خلفية MinIO/S3

للنشر بدون بنية Git التحتية:

```mermaid
flowchart TD
    A["noa push --remote s3-remote"] --> B["PUT /bucket/snapshots/noa_abc123 (msgpack)"]
    A --> C["PUT /bucket/blobs/&lt;sha256&gt; (بايتات خام)"]
    A --> D["PUT /bucket/trees/&lt;sha256&gt; (msgpack)"]
    A --> E["PUT /bucket/refs/default (نص معرف اللقطة)"]
```

مزايا مقارنة بـ Git remote:
- لا حاجة لترجمة الشجرة/اللقطة (تنسيق noa الأصلي)
- تخزين blob مباشر (بدون عبء ملفات pack)
- متوافق مع S3 (يعمل مع AWS, GCS, MinIO, Cloudflare R2)

## إعدادات المستودعات البعيدة

مخزنة في `.noa/config` (TOML):

```toml
[[remotes]]
name = "origin"
url = "https://github.com/example/repo.git"
backend = "git"

[[remotes]]
name = "s3"
url = "s3://my-bucket/noa-repo"
backend = "minio"
endpoint = "https://s3.amazonaws.com"
region = "us-east-1"
```

## المصادقة

| الخلفية | الطريقة |
|---------|--------|
| Git HTTPS | بيانات الاعتماد من `~/.git-credentials` أو مطالبة |
| Git SSH | وكيل SSH أو ملف مفتاح |
| MinIO/S3 | مفتاح وصول + مفتاح سري (متغيرات بيئة أو إعدادات) |

## مقارنة: طرق التوافق البُعدي

| الطريقة | مستخدمة من قبل | الإيجابيات | السلبيات |
|----------|---------|------|------|
| جسر Git (gix) | noa | توافق عالمي | عبء الترجمة، عدم تطابق SHA-1/SHA-256 |
| بروتوكول أصلي | Git | سريع، بدون ترجمة | يعمل فقط مع Git |
| WebDAV | SVN | معياري HTTP | محدود، خاص بـ SVN |
| REST API | Bitbucket | حديث، مرن | يتطلب خدمة مستضافة |
| تخزين متوافق مع S3 | noa | قابل للتوسع، سحابي أصلي | لا توافق مع Git بدون جسر |

يدعم noa كلاً من جسر Git (للتوافق) و S3 الأصلي (للتوسع).
