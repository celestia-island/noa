# مرجع CLI

## `noa init [path]`

تهيئة مستودع `.noa/` جديد. ينشئ `noa.redb` و `agent-logs/` و `HEAD` و `config`.

```bash
noa init .           # المجلد الحالي
noa init /path/repo  # مسار محدد
```

## `noa status`

عرض مساحة العمل الحالية ولقطة الرأس.

```bash
noa status
# On workspace: default (head: noa_abc123, msg: initial)
```

## `noa log [options]`

عرض تاريخ اللقطات.

| العلامة | الافتراضي | الوصف |
|------|---------|-------------|
| `-w, --workspace` | HEAD الحالي | تصفية حسب مساحة العمل |
| `-l, --limit` | 20 | الحد الأقصى للمدخلات المعروضة |

```bash
noa log
noa log --workspace feature-1 --limit 50
```

## `noa snapshot <subcommand>`

### `noa snapshot create [-m msg] [-a author]`

إنشاء لقطة من سجل الوكيل لمساحة العمل الحالية.

```bash
noa snapshot create -m "add login feature" -a "agent-001"
```

### `noa snapshot list`

عرض جميع اللقطات عبر مساحات العمل.

### `noa snapshot diff <a> <b>`

عرض الاختلافات على مستوى الملف بين لقطتين.

```bash
noa snapshot diff noa_abc123 noa_def456
```

## `noa workspace <subcommand>`

### `noa workspace create <name> [--agent <id>]`

إنشاء مساحة عمل جديدة متفرعة من HEAD الحالي.

### `noa workspace switch <name>`

تبديل مساحة العمل النشطة (يُحدّث HEAD).

### `noa workspace list`

عرض جميع مساحات العمل. `*` تشير إلى النشطة.

### `noa workspace delete <name>`

حذف مساحة عمل (لا يمكن حذف مساحة العمل النشطة).

### `noa workspace merge <from>`

دمج مساحة عمل أخرى في المساحة الحالية باستخدام الدمج الثلاثي.

```bash
noa workspace switch default
noa workspace merge feature-1
```

## `noa remote <subcommand>`

### `noa remote add <name> <url>`

إضافة مستودع بعيد.

### `noa remote remove <name>`

إزالة مستودع بعيد.

### `noa remote list`

عرض جميع المستودعات البعيدة المُعدّة.

## `noa push [--remote name]`

الدفع إلى مستودع بعيد (غير منفذ بعد).

## `noa pull [--remote name]`

السحب من مستودع بعيد (غير منفذ بعد).

## `noa fetch [--remote name]`

الجلب من مستودع بعيد بدون دمج (غير منفذ بعد).

## `noa clone <url> [path]`

استنساخ مستودع بعيد (غير منفذ بعد).
