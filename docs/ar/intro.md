<p align="center"><img src="https://raw.githubusercontent.com/celestia-island/docs.celestia.world/dev/res/logo/noa.webp" alt="noa" width="200" /></p>

<h1 align="center">Noa</h1>

<p align="center"><strong>نظام تحكّم بالإصدارات موزّع ومصمَّم في جوهره للوكلاء</strong></p>

<div align="center">

[![License](https://img.shields.io/badge/license-SySL%201.0-blue)](../../LICENSE)
[![GitHub](https://img.shields.io/badge/github-celestia--island%2Fnoa-blue.svg)](https://github.com/celestia-island/noa)

</div>

<div align="center">

[English](../en/intro.md) ·
[简体中文](../zh-Hans/intro.md) ·
[繁體中文](../zh-Hant/intro.md) ·
[日本語](../ja/intro.md) ·
[한국어](../ko/intro.md) ·
[Français](../fr/intro.md) ·
[Español](../es/intro.md) ·
[Русский](../ru/intro.md) ·
**العربية**

</div>

عزل مساحة العمل لكل وكيل، ولقطات معنونة بالمحتوى عبر SHA-256، وخادم متكامل لبروتوكول git الذكي (clone/fetch/push)، ومنصة استضافة ذاتية (forge) مدمج فيها تنظيم المرآة والتدفق العكسي، ودمج ثلاثي الاتجاه على مستوى الأسطر مع كشف دقيق لإعادة التسمية، وصلاحيات RBAC متعددة المستخدمين، وواجهة API لسجلات تشغيل CI — وكل ذلك في ملف تنفيذي واحد مع تخزين مضمّن.

## الوثائق

الوثائق الإنجليزية الكاملة موجودة في هذا المستودع ضمن `docs/en/` (وهذا الموقع ينسخها كمرآة على [noa.docs.celestia.world](https://noa.docs.celestia.world)):

| الوثائق | المحتوى |
|---|---|
| [البدء السريع](./guide/getting-started.md) | البناء، تشغيل الخادم، أول clone/push |
| [نظرة عامة على المعمارية](./guide/architecture.md) | نموذج مساحة العمل / اللقطات / المراجع، مكونات الخادم |
| [المرآة والتدفق العكسي](./guide/mirror.md) | مساران متوازيان، رؤوس PR، التدفق العكسي و canary، التحقق من المطابقة |
| [الصلاحيات والمستخدمون](./guide/rbac.md) | الأدوار، الرموز (tokens)، التطبيق عبر قناتي REST و git |
| [بروتوكول git-http](./guide/git-http.md) | نقاط نهاية البروتوكول الذكي، دلالات push، التفاوض على البروتوكول |
| [الدمج والتعارض](./guide/merge.md) | diff3 على مستوى الأسطر، كشف دقيق لإعادة التسمية، دلالات التبسيط عند الفشل |
| [سجلات تشغيل CI](./guide/ci-runs.md) | واجهة API للسجلات، التكامل مع chest |
| [مرجع HTTP API](./reference/http-api.md) | جميع نقاط نهاية REST |
| [مرجع CLI](./reference/cli.md) | سطح سطر الأوامر الكامل |
| [النشر](./reference/deploy.md) | systemd، متغيرات البيئة، ثالوث النشر |

المصدر المرجعي للصينية المبسّطة موجود في `docs/zh-Hans/`؛ أما اللغات الأخرى فتُترجَم منه على دفعات.

المصدر: [noa](https://github.com/celestia-island/noa).
