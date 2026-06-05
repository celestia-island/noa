# الهندسة المعمارية

## المكونات الأساسية

### ObjectStore

تخزين معنون بالمحتوى للـ blobs والأشجار. يُعنون المحتوى بتجزئة SHA-256.

```
BlobId = SHA256(content)
TreeId = SHA256(msgpack(TreeEntries))
```

التنفيذات:
- **RedbObjectStore**: تخزين محلي باستخدام مخزن KV المدمج redb
- **MinioObjectStore**: تخزين عن بعد باستخدام MinIO المتوافق مع S3

### AgentLog

سجل إلحاقي لكل مساحة عمل للكتابة المتزامنة بدون أقفال. كل مساحة عمل تحصل على ملف JSONL خاص بها تحت `.noa/agent-logs/<ws>.log`.

العمليات:
- **write**: تسجيل كتابة ملف مع مرجع blob
- **delete**: تسجيل حذف ملف
- **rename**: تسجيل إعادة تسمية ملف
- **snapshot**: تسجيل إنشاء لقطة
- **merge**: تسجيل دمج من مساحة عمل أخرى

### Snapshot

حالة غير قابلة للتغيير لحظية لمساحة عمل. تحتوي على تجزئة شجرة، ولقطات آباء، ومؤلف، ورسالة.

```
Snapshot = {
    id: "noa_<12-char-base62>"
    tree_hash: SHA256 لمحتوى الشجرة
    parents: [SnapshotId, ...]
    workspace: اسم مساحة العمل
    author: معرف الوكيل
    timestamp: دقة الميكروثانية
    message: وصف مقروء بشريًا
}
```

### Workspace

سياق عمل معزول لوكيل. يتتبع لقطة الرأس ولقطة القاعدة.

### RefStore

مؤشرات مسماة إلى لقطات مع دلالات مقارنة-وتبديل (CAS) للتحديثات المتزامنة الآمنة.

### محرك الدمج (Merge Engine)

دمج ثلاثي يقارن أشجار base و ours و theirs:
- نفس التغيير في كلا الجانبين → لا تعارض
- تغيير في جانب واحد فقط → تطبيق
- تغييرات مختلفة لنفس الملف → تعارض (الافتراضي: upstream-wins)

## تخطيط التخزين

```mermaid
graph TD
    NOA[".noa/"] --> DB["noa.redb<br/>(قاعدة بيانات redb: blobs, trees, snapshots, workspaces, refs)"]
    NOA --> LOGS["agent-logs/"]
    LOGS --> LOG1["&lt;ws&gt;.log<br/>(JSONL لكل مساحة عمل)"]
    NOA --> HEAD["HEAD<br/>(اسم مساحة العمل الحالية)"]
    NOA --> ORIG["ORIG_HEAD<br/>(اسم مساحة العمل السابقة)"]
    NOA --> CFG["config<br/>(إعدادات TOML)"]
```

## تدفق البيانات

```mermaid
flowchart TD
    A["الوكيل يكتب"] --> B["AgentLog (JSONL, O_APPEND)"]
    B --> C["SnapshotEngine.compute()"]
    C --> D["بناء شجرة من عمليات write/delete/rename"]
    D --> E["تخزين الشجرة → ObjectStore"]
    E --> F["إنشاء Snapshot → SnapshotStore"]
    F --> G["تحديث رأس مساحة العمل → WorkspaceManager"]
```
