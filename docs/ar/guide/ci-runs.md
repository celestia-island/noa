# سجلات تشغيل CI (واجهة runs API)

يحتوي noa على تخزين مدمج لسجلات تشغيل CI: «مسار تحققات نُفّذ ضد مستودع ما عند رأس معين» هو run واحد. وهذا هو مصدر البيانات لمسار `CI_BACKEND=noa` في واجهة CI للوحة DevOps في chest.

## نقاط النهاية

| الطريقة | المسار | الوصف |
|---|---|---|
| POST | `/api/v1/repos/{name}/runs` | يسجّل run واحدة (دور الكتابة) |
| GET | `/api/v1/repos/{name}/runs?limit=&offset=` | قائمة، الأحدث أولًا |

- يجب أن يكون `{name}` موجودًا في الفهرس (وإلا 404)؛
- `number` يخصّه الخادم لكل مستودع (ذرّيًا: المسح والكتابة يجريان في معاملة كتابة واحدة، فلا تستبدل السجلات المتزامنة أرقام بعضها أبدًا)؛
- التحقق: `head_sha` من 40 حرفًا سداسيًا عشريًا؛ و`status` ∈ `queued | running | success | failure | cancelled`؛ و`kind` من 1..=64 حرفًا؛ و`actor` من 1..=200؛ و`log_ref` / `summary` ≤ 2000.

## تسجيل run

```bash
curl -X POST -H "Authorization: Bearer $TOKEN" -H "Content-Type: application/json" \
  -d '{
    "head_sha": "<40-hex>",
    "kind": "basic-check",
    "status": "success",
    "actor": "farm-runner-1",
    "log_ref": "https://ci.example.com/logs/123"
  }' \
  http://127.0.0.1:3016/api/v1/repos/noa/runs
```

الاستجابة (201): يملأ الخادم `repo` و `number` و `started_us` (عندما لم يزوّده العميل).

## الاستعلام

```bash
curl -H "Authorization: Bearer $TOKEN" \
  "http://127.0.0.1:3016/api/v1/repos/noa/runs?limit=10&offset=0"
```

الصفوف المعادة تشبه:

```json
{
  "repo": "noa",
  "number": 1,
  "head_sha": "…",
  "kind": "basic-check",
  "status": "success",
  "actor": "farm-runner-1",
  "started_us": 1794049465562618
}
```

## حدود التصميم

- **السجلات لا تُخزَّن في noa**: تحمل السجلات مؤشر `log_ref` غير شفاف إلى مخزن السجلات الخاص بمسار الإرسال؛
- **محتوى الصف له حدود** (انظر التحقق) — لمنع الكتّاب المصادَقين من تضخيم حجم الصفوف؛
- **التخزين**: جدول `runs` في redb، بمفتاح `{repo}#{number:020}` (الترتيب المعجمي = الترتيب الزمني داخل المستودع).

## التكامل مع chest

يقرأ معالج `ci_ops` في chest متغير البيئة `CI_BACKEND`:

- `github` (الافتراضي): مسار GitHub Actions v1، سلوكه مطابق للإصدارات التاريخية؛
- `noa`: يقرأ واجهة runs API في noa المشار إليها عبر `NOA_API_BASE` + `NOA_API_TOKEN`، ويوحّد الصفوف إلى شكل اللوحة نفسه الذي تظهر به صفوف GitHub (`kind`→name، `number`→run_number، `started_us`→`created_at` بصيغة RFC3339، والحالات النهائية تُساق إلى conclusion)؛ وقائمة المراقبة تستخدم `CI_NOA_REPOS` (أسماء مجلدات بمقطع واحد)، وترجع عند غيابه إلى المقطع الأخير من `CI_GITHUB_REPOS`.

يبلّغ حقل `backend` في `ci.config.get` للوحة عن الواجهة الخلفية؛ و`configured` تراعي الواجهة الخلفية — النشر الخالص لـ noa (بلا رمز GitHub) يُعد متصلًا ما دام ثالوث noa مكتملًا.
