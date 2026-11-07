# النشر

شكل نشر noa-server: ثنائي واحد + قاعدة بيانات redb واحدة + وحدة systemd واحدة.
النسخ الإنتاجية يشرف عليها malkuth.

## ثالوث النشر

1. **الملف الناتج**: انسخ مخرجات `cargo build --release --locked --bin noa-server`
   إلى مسار النشر (استبدال ذري: اكتب `<bin>.new` ثم rename)، مع التحقق من sha256 على
   الجانبين؛
2. **إعادة محاولة health**: استطلع `/api/v1/*` بعد الإقلاع (401 = العملية حية
   والمصادقة تعمل)؛
3. **التراجع**: احتفظ بمسار الإصدار السابق؛ `mv` للرجوع + إعادة تشغيل.

## متغيرات البيئة (إعدادات الإقلاع الفئة A → ملف env بصلاحيات 0600)

| المتغير | الوصف |
|---|---|
| `NOA_API_TOKEN` | توكن المشرف التمهيدي؛ **القيمة الفارغة ترفض الإقلاع** |
| `NOA_MIRROR_ENABLED` | مفتاح مجدول المرآة |
| `NOA_MIRROR_INTERVAL_SECS` | فاصل الموجات (الافتراضي 900) |
| `NOA_MIRROR_SCRATCH_DIR` | جذر scratch (مثل `/var/lib/noa/mirror-cache`) |
| `NOA_MIRROR_GH_TOKEN` | بيانات كتابة GitHub للـ back-flow / canary (اسم token_env الافتراضي) |
| `NOA_MIRROR_GH_TOKEN_ENV` | يغيّر اسم متغير البيانات (الافتراضي هو السطر أعلاه) |

بيانات الاعتماد تعيش فقط في مستوى النشر (env 0600 / systemd LoadCredential) —
أبداً في قاعدة بيانات أو شجرة git أو مستند.

## وحدة systemd

```ini
[Service]
EnvironmentFile=/etc/noa/noa-server.env
ExecStart=/usr/local/bin/noa-server --host 0.0.0.0 --port 3016
Restart=on-failure
```

- الاستماع على `0.0.0.0` (عادة عقد العائلة: bearer على كل المسارات + إنفاذ دور
  الكتابة يبقي التعرض آمناً)؛
- بعد تغيير env: `systemctl daemon-reload && systemctl restart`؛ تحقق عبر
  `tr '\0' '\n' < /proc/$MAINPID/environ`؛
- النسخ الإنتاجية بإشراف malkuth (ادفع مطالبة النشر الحصرية قبل لمس مسارات النشر).

## المنافذ والنطاقات

تخصيص المنافذ في دفتر مساحة العمل (`_ledger/ports-and-domains.md`)؛ الإنتاج
على 3016 مع nginx أمامياً منهي TLS.

## عدة نسخ

redb مكتبة ملف واحد — مثيل واحد يملك ملف قاعدة بيانات واحد. عدة نسخ = عدة
مجلدات ومنافذ (مثل وضع النسختين `chest-demo` + chest الإنتاجي)؛ نقل الإعدادات
بين النسخ عبر RPC import / export، أبداً بنسخ ملفات الإعداد.

## التحقق بعد النشر

```bash
# المصادقة والجهوزية
curl -s -o /dev/null -w '%{http_code}\n' http://127.0.0.1:3016/api/v1/prs   # المتوقع 401
# موجة مرآة
curl -X POST -H "Authorization: Bearer $T" -H "Content-Type: application/json" \
  -d '{"job":"noa"}' http://127.0.0.1:3016/api/v1/mirror/run               # المتوقع 202
```

تفاصيل المرآة ودلالات الـ back-flow في
[المرآة والـ back-flow](../guide/mirror.md).
