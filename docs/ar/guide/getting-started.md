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

## سير العمل الأصلي في noa (موصى به)

أداة `noa` في سطر الأوامر هي المدخل من الدرجة الأولى — ما تستنسخه مستودعٌ يتعايش فيه
`.git` مع `.noa` (يتولى git إدارة المصدر، ويتولى noa إدارة بيانات الوكلاء):

```bash
noa clone http://<token>@127.0.0.1:3016/git/hello --path hello
cd hello
noa workspace create agent-a     # a per-agent isolated workspace
# … edit files …
noa snapshot create -m "change" -a agent-a
noa log                          # this workspace's snapshot history
noa push                         # publishes the workspace snapshot (serialized to a git commit)
```

ينشر `noa push` **حالة لقطة مساحة العمل** (يُنشئ commit من نوع `[noa export]`)، وليس
commits الموجودة في `.git`. المساران متوازيان؛ لا تخلط بينهما (نفّذ `noa snapshot create`
قبل الـ push وإلا نشرتَ لقطة قديمة). تُحقن بيانات الاعتماد من متغيري البيئة
`NOA_GIT_USER` + `NOA_GIT_PASS`. الدليل بأكمله (بما فيه `.git/noa`) يُنسخ إلى آلة أخرى
ويواصل العمل كما هو.

## مسار التوافق مع git (أي أداة git تعمل)

نقطة النهاية `/git/{repo}` نفسها تتحدث أيضًا ببروتوكول git الذكي القياسي عبر HTTP —
نسخة git الخام وCI وfarm تعمل دون أي تعديل:

```bash
git clone http://<token>@127.0.0.1:3016/git/hello
```

تتطلب عمليات push (receive-pack) دور الكتابة؛ والـ push الذي يصل **يُطلق موجة الاستيراد
فورًا** (ويأتي التدفق العكسي في الموجة نفسها). هذه هي الصورة الانتقالية التي تُبقي
farm/CNB دون أي تغيير.

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

## النسخ الاحتياطي والترحيل

يمكن تصدير حالة الخادم بأكملها إلى دليل واحد قابل للنقل (انظر قسم النسخ الاحتياطي في
[النشر](../reference/deploy.md)): لقطة redb متسقة + حزمة git قابلة للاستنساخ لكل مستودع +
بيان sha256؛ ويعيد `noa-server --import-state <dir>` الاستيراد دون اتصال. معيار القبول هو
التمرين: تصدير → استيراد على آلة أخرى → تشغيل → مطابقة.


## الخطوات التالية

- [نظرة عامة على المعمارية](./architecture.md) — نموذج مساحة العمل / اللقطات / المراجع
- [المرآة والتدفق العكسي](./mirror.md) — مساران متوازيان، رؤوس PR، التدفق العكسي و canary
- [مرجع HTTP API](../reference/http-api.md) — جميع نقاط النهاية
