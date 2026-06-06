# اختيار الواجهة الخلفية لـ Git: gix (gitoxide) مقابل git2 (libgit2)

## الحالة: تحليل

**الحالي**: `git2 = "0.19"` (ربط C بـ libgit2)
**المقترح**: `gix = "0.84"` (تنفيذ Git خالص بلغة Rust)

## ملخص

gix (gitoxide) هو تنفيذ Git ناضج وخالص بلغة Rust مع تغطية ميزات كافية لاستبدال git2 في جسر Git الخاص بـ noa. يلغي هذا الانتقال اعتمادية C (libgit2)، ويقلل من احتكاك الترجمة المتقاطعة (cross-compilation)، ويوفر واجهات برمجة Rust اصطلاحية.

## مصفوفة المقارنة

| المعيار | git2 (libgit2) | gix (gitoxide) |
|-----------|---------------|----------------|
| **اللغة** | C (روابط Rust عبر مكتبة git2) | Rust خالص |
| **النضج** | 14 سنة، مُثبت في الإنتاج | 5 سنوات، تطوير نشط (0.84) |
| **الترجمة** | ~15 ثانية (إعادة بناء)، يتطلب CMake + libgit2-dev | ~8 ثوانٍ (إعادة بناء)، cargo فقط |
| **الترجمة المتقاطعة** | صعبة (تحتاج سلسلة أدوات C متقاطعة) | بسيطة (cargo cross-compile) |
| **نمط API** | شبيه بـ C، كتل unsafe، أعمار يدوية | اصطلاحي Rust، آمن استعاريًا، أنماط بناء |
| **معالجة الكائنات** | git2::Blob, Tree, Commit عبر ODB | gix::objs::BlobRef, TreeRef, CommitRef |
| **استعراض الشجرة** | مكرر يدوي مع .to_object() | breadthfirst/virtual_roots مع مفوّض |
| **الدفع/السحب عن بُعد** | git2::Remote (fetch, push) | gix::remote (connect, fetch, push) |
| **Pack/pack-index** | مدمج | شامل (مكتبة مستقلة: gix-pack) |
| **المراجع (Refs)** | git2::Reference (قراءة/كتابة) | gix::refs (دعم كامل للمعاملات) |
| **الإعدادات (Config)** | محدود (على مستوى المستودع) | إعدادات متعددة الطبقات (النظام، المستخدم، المستودع) |
| **SHA-1/256** | SHA-1 فقط | SHA-1 + SHA-256 (تجريبي) |
| **أمان الذاكرة** | خطر من أخطاء libgit2 C | ضمانات Rust |
| **قابلية التدقيق** | تحتاج لتدقيق شيفرة libgit2 C | Rust فقط، cargo-audit |
| **المجتمع** | ضخم (جميع أدوات VCS الرئيسية) | نامٍ (gitoxide, crates-index-diff، إلخ) |

## متطلبات جسر Git في noa

الاستخدام الحالي في `src/git/`:

```rust
// import.rs:
//   - Repository::open()           → gix::open()
//   - repo.head().target()         → gix.head().project_id()
//   - repo.find_commit(oid)        → gix.find_object().try_into_commit()
//   - commit.tree()                → gix.find_object(commit.tree()).try_into_tree()
//   - tree.iter()                  → gix::objs::TreeRefIter
//   - entry.to_object(repo)        → gix.find_object(entry.oid())
//   - obj.kind() === Blob          → obj.kind == ObjectKind::Blob
//   - blob.content()               → blob.data

// translate.rs:
//   - معالجة على مستوى البايتات فقط (بدون اعتمادية git خارجية)

// export.rs:
//   - حاليًا todo!() — الدفع سيستخدم gix::remote::connect()
//   - توليد ملفات pack عبر gix-pack (إذا لزم الأمر)
```

جميع استدعاءات API الستة الحالية لها مكافئات مباشرة في gix.

## تغطية ميزات gix لـ noa

| الميزة المطلوبة | دعم git2 | دعم gix | ملاحظات |
|---------------|-------------|-------------|-------|
| فتح مستودع | ✅ | ✅ | `gix::open()` أو `gix::ThreadSafeRepository::open()` |
| قراءة مرجع HEAD | ✅ | ✅ | `gix.head_ref()` / `gix.head()` |
| إيجاد commit بواسطة OID | ✅ | ✅ | `gix.find_object(id)?.try_into_commit()` |
| قراءة شجرة من commit | ✅ | ✅ | `gix.find_object(commit.tree())?.try_into_tree()` |
| تكرار مدخلات الشجرة | ✅ | ✅ | `tree.iter()` يعيد `TreeRefIter` |
| قراءة محتوى blob | ✅ | ✅ | `blob.data` على `BlobRef` |
| الجلب من مستودع بعيد | ✅ | ✅ | `gix::remote::connect()`  |
| الدفع إلى مستودع بعيد | ✅ | ✅ | `gix::remote::connect()`  |
| استنساخ | ✅ | ✅ | `gix::prepare_clone()` |
| توليد ملف pack | ✅ | ✅ | مكتبة `gix-pack` |
| دعم SHA-256 | ❌ | ✅ (تجريبي) | ذو صلة بلقطات SHA-256 |
| دعم async | ❌ | ✅ (اختياري) | جيد لتكامل tokio |

## الجدوى

جميع عمليات git الحالية والمخطط لها لها مكافئات في gix. تعيين API مباشر:

```rust
// git2 (الحالي)
let repo = git2::Repository::open(path)?;
let head = repo.head()?;
let commit = repo.find_commit(head.target().unwrap())?;
let tree = commit.tree()?;

// gix (المقترح)
let repo = gix::open(path)?;
let head = repo.head_ref()?.expect("HEAD not found");
let head_id = head.id().detach();
let commit = repo.find_object(head_id)?.try_into_commit()
    .map_err(|_| NoaError::Remote("not a commit".into()))?;
let tree = repo.find_object(commit.tree())?.try_into_tree()
    .map_err(|_| NoaError::Remote("not a tree".into()))?;
```

## خطة الانتقال

### المرحلة 1: استبدال import.rs (عمليات القراءة فقط)
- استبدال git2::Repository بـ gix::ThreadSafeRepository
- إعادة تنفيذ استعراض الشجرة
- تشغيل اختبارات استيراد git الحالية

### المرحلة 2: استبدال translate.rs
- لا تغييرات مطلوبة (معالجة بايتات خالصة، بدون اعتمادية C)

### المرحلة 3: تنفيذ export.rs عبر gix
- استخدام gix::remote للدفع
- استخدام gix::prepare_clone للاستنساخ
- استخدام gix-pack لتوليد packfile (إذا لزم الأمر لجانب الخادم)

### المرحلة 4: إزالة git2 من Cargo.toml
- إسقاط اعتمادية libgit2 النظامية
- التحقق من الترجمة المتقاطعة (x86_64 → aarch64، → wasm مستقبلًا)

## تقييم المخاطر

| الخطر | الاحتمالية | التأثير | التخفيف |
|------|-----------|--------|------------|
| تغييرات كسر API في gix (0.x) | متوسطة | منخفض | تثبيت الإصدار، التكيف مع تغييرات API |
| ميزات متقدمة مفقودة | منخفضة | متوسط | gix يدعم remote push/fetch منذ 0.50+ |
| تراجع في الأداء | منخفض | منخفض | gix غالبًا أسرع (بدون عبء C FFI) |
| خطر تبني المجتمع | منخفض | منخفض | gix هي مكتبة Rust الفعلية لـ Git |
| أخطاء توافق SHA-256 | متوسطة | منخفض | معزولة بالميزات، تجاوز عبر translate.rs الخالص |

## التوصية

**الانتقال إلى gix.** الفوائد (صفر اعتماديات C، أمان Rust الخالص، ترجمة متقاطعة أسهل، دعم SHA-256) تفوق المخاطر (استقرار API في 0.x، مجتمع أصغر). الانتقال منخفض المخاطر لأن:

1. استخدام git2 الحالي محدود (6 استدعاءات API في import.rs)
2. translate.rs لا يتطلب تغييرات
3. export.rs غير منفذ (أرض بكر لـ gix)
4. gix هي مكتبة Git القياسية في Rust (تستخدمها crates.io index)

## الاعتماديات بعد الانتقال

```diff
- git2 = "0.19"           # ربط libgit2 C
+ gix = { version = "0.84", features = ["basic", "index", "pack"] }
```

لا اعتماديات نظام جديدة. `cargo build` خالص.
