# بناء noa

## المتطلبات الأساسية

- Rust 1.75+ (مستقر)
- Python 3.8+ (لسكربتات البناء)
- مشغّل الأوامر `just`

## الإعداد

```bash
git clone https://github.com/celestia-island/noa.git
cd noa
just init          # جلب اعتماديات Rust
just build-dev     # بناء تطويري
```

## التطوير

```bash
just fmt            # تنسيق الشيفرة
just clippy         # تدقيق الشيفرة
just test           # تشغيل الاختبارات
just check          # فحص الأنواع
```

## هيكل المشروع

```mermaid
graph TD
    SRC["src/"] --> LIB["lib.rs<br/>(جذر المكتبة)"]
    SRC --> ERR["error.rs<br/>(أنواع الأخطاء)"]
    SRC --> CFG["config.rs<br/>(الإعدادات)"]
    SRC --> REPO["repo.rs<br/>(دورة حياة المستودع)"]
    SRC --> OBJ["object/<br/>(واجهة ObjectStore + تنفيذات)"]
    SRC --> LOG["log/<br/>(واجهة AgentLog + تنفيذات)"]
    SRC --> SNAP["snapshot/<br/>(محرك اللقطات)"]
    SRC --> WS["workspace/<br/>(مدير مساحات العمل)"]
    SRC --> REFS["refs.rs<br/>(واجهة RefStore + تنفيذ)"]
    SRC --> MERGE["merge/<br/>(محرك الدمج)"]
    SRC --> GIT["git/<br/>(توافق Git)"]
    SRC --> REMOTE["remote.rs<br/>(واجهة RemoteBackend)"]
    SRC --> CLI["cli/<br/>(أوامر CLI)"]
```
