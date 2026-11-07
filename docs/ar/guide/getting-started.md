# البدء السريع

تأخذك هذه الصفحة من الصفر إلى noa يعمل فعليًا: البناء، ثم تشغيل الخادم، ثم إتمام أول clone / push.

## المتطلبات المسبقة

- Rust stable ≥ 1.99 (يُثبَّت عبر `rustup`)
- git ≥ 2.39 (البروتوكول الذكي عبر HTTP للخادم يشغّل `upload-pack` / `receive-pack` مباشرة)
- اختياري: node + pnpm (لبناء WebUI)

## البناء

```bash
git clone https://github.com/celestia-island/noa.git
cd noa
cargo build --release --locked
```

المنتجات: `target/release/noa` (سطر الأوامر CLI) و `target/release/noa-server` (الخادم).

## تشغيل الخادم

تحتاج الإعدادات الدنيا إلى رمز API واحد فقط (بيانات اعتماد المدير التمهيدية):

```bash
export NOA_API_TOKEN="<your-token>"
export NOA_MIRROR_SCRATCH_DIR=/var/lib/noa/mirror-cache   # mirror scratch root
export NOA_MIRROR_ENABLED=1                               # enable mirror scheduling (default 900s)
export NOA_MIRROR_GH_TOKEN="<github-pat>"                 # optional: back-flow / canary push credential
noa-server --host 0.0.0.0 --port 3016
```

بعد التشغيل:

```bash
curl -H "Authorization: Bearer <your-token>" http://127.0.0.1:3016/api/v1/repos
```

استجابة `[]` تعني أن الخدمة جاهزة. جميع نقاط نهاية REST تتطلب مصادقة من نوع bearer؛ والشيء نفسه ينطبق على مسارات القراءة تحت `/git/{repo}/*` (الطلبات غير المصادَقة تحصل على 401 مع تحدٍّ من نوع `Basic`).

## أول مرآة (استيعاب مستودع من GitHub داخل noa)

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

بمجرد انتهاء الموجة، يصبح المستودع قابلًا للاستنساخ عبر noa:

```bash
git clone http://<your-token>@127.0.0.1:3016/git/hello
```

## أول push (سلطة الكتابة)

تمر عمليات push عبر مسار receive-pack تحت `/git/{repo}` وتتطلب هوية مصادَقة:

```bash
cd hello
echo "hello noa" > README.md
git add -A && git commit -m "hello noa"
git push origin master
```

يصل الـ push إلى مساحة المرآة المؤقتة، ثم تستوعبه موجة المرآة التالية تلقائيًا داخل نظام اللقطات والمراجع في noa، وإذا كانت الوظيفة قد فعّلت `backflow` فإنه تُدفَع الحالة أيضًا عودةً إلى المصدر (GitHub) — وهي الصورة الانتقالية لمبدأ «noa هو المرجعية + GitHub مرآة».

## الخطوات التالية

- [نظرة عامة على المعمارية](./architecture.md) — نموذج مساحة العمل / اللقطات / المراجع
- [المرآة والتدفق العكسي](./mirror.md) — مساران متوازيان، رؤوس PR، التدفق العكسي و canary
- [مرجع HTTP API](../reference/http-api.md) — جميع نقاط النهاية
