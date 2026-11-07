# الصلاحيات والمستخدمون

مصادقة noa مبنية على مبدأ «رمز واحد، قناتان»: تشترك REST و git في بيانات الاعتماد نفسها، لكن صلاحية الكتابة تُفرض على كل قناة على حدة.

## بيانات الاعتماد

- **الرمز التمهيدي**: متغير البيئة `NOA_API_TOKEN`. يرفض الخادم الإقلاع بقيمة فارغة (القيمة الفارغة / كلها فراغات تُرفض عند التشغيل)؛ وهذا الرمز هو هوية المدير.
- **رموز المستخدمين المسجلين**: تُنشأ عبر REST (انظر أدناه)، وتُخزَّن كتجزئات sha256، وتُقارن بزمن ثابت.

أشكال المصادقة (مطابقة على القناتين):

```text
Authorization: Bearer <token>
Authorization: Basic <base64(any-user:token)>     # git client convention: any username
```

الخادم ذو الرمز الفارغ (`NOA_API_TOKEN=""`) لا يتحول إلى قراءة مجهولة — الإقلاع يُرفض رفضًا قاطعًا، وشرط عدم الفراغ قبل المقارنة يضيف طبقة دفاعية إضافية.

## الأدوار

| الدور | القراءة | الكتابة (REST / git push) | الإدارة (CRUD للمستخدمين) |
|---|---|---|---|
| admin | ✓ | ✓ | ✓ |
| operator | ✓ | ✓ | ✗ |
| reader | ✓ | ✗ | ✗ |

- REST: التطبيق مركّز في الوسيط (middleware) — طلبات الكتابة من reader تحصل على 403؛ وأي طلب غير GET على `/api/v1/users` يحصل على 403 (للمدير فقط).
- git: عملية push (receive-pack) تتطلب can_write — بلا بيانات اعتماد 401 (مع تحدٍّ من نوع `Basic` يستخدمه عملاء git للاستفهام)؛ وببيانات اعتماد موجودة لكنها reader تحصل على 403 (للإشارة إلى أن دور الكتابة مطلوب). عمليات clone/fetch تعمل بأي هوية مصادَقة.

## إدارة المستخدمين عبر REST

```bash
# Create (admin)
curl -X POST -H "Authorization: Bearer $ADMIN" -H "Content-Type: application/json" \
  -d '{"name":"alice","role":"operator","token":"<alice-token>"}' \
  http://127.0.0.1:3016/api/v1/users

# List (admin) / single
curl -H "Authorization: Bearer $ADMIN" http://127.0.0.1:3016/api/v1/users
curl -H "Authorization: Bearer $ADMIN" http://127.0.0.1:3016/api/v1/users/alice

# Delete (admin)
curl -X DELETE -H "Authorization: Bearer $ADMIN" \
  http://127.0.0.1:3016/api/v1/users/alice
```

القواعد:

- `name` و `role` (admin/operator/reader) مطلوبان؛ والخادم يُجزّئ الرمز بـ sha256 قبل تخزينه.
- التحديث (POST إلى المسار نفسه وبالاسم نفسه) يمكنه تدوير الرمز أو تغيير الدور؛ بينما يحتفظ `created_us` بقيمته الأصلية.
- الحذف للمدير فقط ولا رجعة فيه.

## مثال على قناة git

```bash
# Read (any authenticated identity; public repos work with a placeholder username)
git clone http://<token>@noa.example.com:3016/git/noa

# Write (requires operator/admin)
git push origin master
```

عملية push غير مصادَقة تحصل على 401 مع تحدٍّ من نوع `Basic` (يستخدمه عملاء git للاستفهام عن بيانات الاعتماد)؛ وعملية push بهوية reader تحصل على 403 (للإشارة إلى أن دور الكتابة مطلوب).
