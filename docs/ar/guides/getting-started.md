# البدء

## المتطلبات الأساسية

- Rust 1.75+ (مستقر)
- Python 3.8+ (لسكربتات البناء)
- مشغّل الأوامر `just`

## التثبيت

```bash
git clone https://github.com/celestia-island/noa.git
cd noa
just init          # جلب الاعتماديات
just build-dev     # بناء تطويري
```

الملف الثنائي `noa` موجود في `target/debug/noa`.

## البدء السريع

```bash
# تهيئة مستودع جديد
noa init .

# التحقق من الحالة
noa status
# On workspace: default

# إنشاء مساحة عمل
noa workspace create feature-1

# التبديل إليها
noa workspace switch feature-1

# إنشاء لقطة
noa snapshot create -m "initial work"

# عرض التاريخ
noa log

# التبديل مرة أخرى والدمج
noa workspace switch default
noa workspace merge feature-1

# إدارة المستودعات البعيدة
noa remote add origin https://github.com/example/repo.git
noa remote list
```

## تشغيل الأمثلة

```bash
python3 examples/run_all.py
```

## التطوير

```bash
just fmt            # تنسيق الشيفرة
just clippy         # تدقيق الشيفرة
just test           # تشغيل الاختبارات
just check          # فحص الأنواع
```
