<div align="center">

# AT-Switch

### أداة شاملة لإدارة وتبديل النماذج لـ WorkBuddy و CodeBuddy و QClaw و AutoClaw و Codex

[![Version](https://img.shields.io/github/v/release/atswitch/at-switch?color=blue&label=version)](https://github.com/atswitch/at-switch/releases)
[![Platform](https://img.shields.io/badge/platform-macOS%20%7C%20Windows-lightgrey.svg)](https://github.com/atswitch/at-switch/releases)
[![Built with Tauri](https://img.shields.io/badge/built%20with-Tauri%202-orange.svg)](https://tauri.app/)
[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](https://opensource.org/licenses/MIT)
[![Downloads](https://img.shields.io/github/downloads/atswitch/at-switch/total)](https://github.com/atswitch/at-switch/releases/latest)

### 🌐 الموقع الرسمي الوحيد: **[atswitch.io](https://atswitch.io)**

[中文](README.md) | [English](README_EN.md) | [日本語](README_JA.md) | العربية | [سجل التحديثات](CHANGELOG.md)

</div>

---

> [!WARNING]
>
> ## بيان القنوات الرسمية (يرجى القراءة بعناية)
>
> إن AT-Switch تطبيق مكتبي **مجاني ومفتوح المصدر بالكامل**، و**لا يفرض أي رسوم مطلقاً**. يُرجى الحصول على البرنامج حصرياً عبر القنوات الرسمية التالية:
>
> | الفئة | الرابط الرسمي |
> | :--- | :--- |
> | **الموقع الرسمي** | **[atswitch.io](https://atswitch.io)** |
> | **الكود المصدري** | **[github.com/atswitch/at-switch](https://github.com/atswitch/at-switch)** |
> | **التنزيلات** | **[GitHub Releases](https://github.com/atswitch/at-switch/releases)** |
> | **المشكلات والملاحظات** | **[GitHub Issues](https://github.com/atswitch/at-switch/issues)** |
>
> **أي موقع أو تطبيق يطلب الدفع أو الشحن أو يطلب بيانات تسجيل الدخول الشخصية باسم "AT-Switch" هو احتيالي.**

---

## نبذة عن البرنامج

**AT-Switch** أداة مكتبية محلية لنظامي macOS و Windows لتهيئة وتبديل مزوّدي ونماذج الذكاء الاصطناعي (LLM) بسهولة عبر وكلاء البرمجة المتعددين.

بدلاً من البحث في ملفات التكوين المشتتة لكل تطبيق، يوحد AT-Switch سير العمل في مسار مكتبي واضح: **اختيار الوكيل ← إدارة المزوّد ← اختيار النموذج ← التبديل الفوري**.

- **الاتصال المباشر أولاً**: يقوم التطبيق افتراضياً بتعديل ملفات تكوين الوكلاء محلياً وبشكل مباشر دون أي تأخير أو وسيط.
- **الوكيل المحلي (Local Proxy)**: عند الحاجة إلى تحويل البروتوكولات أو عزل مفاتيح API، يمكن تفعيل الوكيل المحلي بنقرة واحدة من الإعدادات المتقدمة.
- **أمان وخصوصية محلية**: مبني باستخدام Tauri 2 و Rust و React و TypeScript. تُحفظ مفاتيح API الحساسة في خزائن بيانات الاعتماد الأصلية للنظام (macOS Keychain أو Windows Credential Manager)، ولا يتم أبداً جمع أو تسجيل مدخلات Prompt أو ردود النماذج.

---

## ✨ الميزات الرئيسية

- **إدارة مركزية للمزوّدين والنماذج**: دعم DeepSeek و Moonshot Kimi و Zhipu GLM و Doubao و MiniMax و Qwen وغيرها من النماذج الرائدة ونقاط النهاية المتوافقة المخصصة.
- **إعداد مستقل لكل وكيل**: يحتفظ كل وكيل بمزوّد ونموذج مستقل ووضع اتصال مباشر أو عبر الوكيل المحلي.
- **تحويل متعدد البروتوكولات في الاتجاهين**: دعم التحويل المتوافق بين بروتوكولات **OpenAI Chat Completions** و **OpenAI Responses** و **Anthropic Messages**.
- **دعم البث واستدعاء الأدوات**: وحدة معالجة مدمجة تحافظ على البث المباشر (SSE) واستدعاء الدوال والوظائف أثناء تحويل البروتوكولات.
- **أمان معاملات التكوين**: نسخ احتياطي مشفر قبل كل كتابة، مع استبدال ذري والتحقق من صحة البيانات والاسترجاع التلقائي عند أي خطأ.
- **التعرف التلقائي على الوكلاء**: اكتشاف تلقائي للوكلاء المثبتين ودعم إعادة التشغيل الآمن للعمليات الجارية عند تغيير التكوين.
- **استعادة الحالة الأصلية بنقرة واحدة**: إمكانية إلغاء الإدارة واستعادة التكوين الأصلي لكل وكيل في أي وقت.

---

## 💻 المنصات المدعومة وتنزيل الحزم

يتم نشر جميع الإصدارات الرسمية عبر [GitHub Releases](https://github.com/atswitch/at-switch/releases).

| المنصة | الحد الأدنى للنظام | المعمارية | صيغة التثبيت |
| :--- | :--- | :--- | :--- |
| **macOS** | macOS 12 Monterey فما فوق | Apple Silicon / Intel / Universal | ملف `.dmg` |
| **Windows** | Windows 10 / 11 | x64 | مثبت `.msi` / النسخة المحمولة (`.zip`) |

---

## 🤖 مصفوفة الوكلاء المدعومين

| الوكيل | الاكتشاف التلقائي | الوضع المباشر | بروتوكول الطلب الافتراضي | آلية التحديث |
| :--- | :--- | :--- | :--- | :--- |
| **WorkBuddy** | macOS / Windows | ✅ مدعوم | OpenAI Chat Completions | تحديث `~/.workbuddy/models.json` مع الحفاظ على التخصيصات |
| **CodeBuddy CN** | macOS / Windows | ✅ مدعوم | OpenAI Chat Completions | تحديث `~/.codebuddy/models.json` ومزامنة الإعدادات الافتراضية |
| **QClaw** | macOS / Windows | ✅ مدعوم | OpenAI Chat Completions | تحديد موقع ومزامنة تكوين OpenClaw عبر `~/.qclaw/qclaw.json` |
| **AutoClaw** | macOS / Windows | ✅ مدعوم | OpenAI Chat Completions | إدارة إعدادات النماذج في دليل بيانات Electron |
| **Codex** | macOS / Windows | ✅ مدعوم | OpenAI Responses | تحديث `$CODEX_HOME/config.toml` أو `~/.codex/config.toml` بدقة |

---

## 🛠️ البناء والتطوير المحلي

### المتطلبات الأساسية
- [Node.js](https://nodejs.org/) (>= 20)
- [Rust](https://www.rust-lang.org/) (الإصدار المستقر)
- متطلبات بناء النظام:
  - macOS: أدوات سطر أوامر Xcode
  - Windows: أدوات بناء Visual Studio C++ و WebView2 Runtime

### التشغيل والتطوير

```bash
# استنساخ المستودع
git clone https://github.com/atswitch/at-switch.git
cd at-switch

# تثبيت الاعتماديات
npm ci

# تشغيل بيئة التطوير المكتبي
npm run tauri dev
```

### فحص الجودة والاختبارات

```bash
# بناء الواجهة والاختبارات الآلية
npm run build
npm test -- --run

# فحص التنسيق و Clippy في Rust
cargo fmt --manifest-path src-tauri/Cargo.toml -- --check
cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets -- -D warnings

# اختبارات وحدة وتكامل Rust
cargo test --manifest-path src-tauri/Cargo.toml
```

---

## 📄 رخصة المشروع

هذا المشروع مفتوح المصدر تحت [رخصة MIT](LICENSE).
