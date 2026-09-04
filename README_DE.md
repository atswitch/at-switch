<div align="center">

# AT-Switch

### Der All-in-One Manager & Modell-Umschalter für Codex, WorkBuddy, CodeBuddy, QClaw & AutoClaw

[![Version](https://img.shields.io/github/v/release/atswitch/at-switch?color=blue&label=version)](https://github.com/atswitch/at-switch/releases)
[![Platform](https://img.shields.io/badge/platform-macOS%20%7C%20Windows-lightgrey.svg)](https://github.com/atswitch/at-switch/releases)
[![Built with Tauri](https://img.shields.io/badge/built%20with-Tauri%202-orange.svg)](https://tauri.app/)
[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](https://opensource.org/licenses/MIT)
[![Downloads](https://img.shields.io/github/downloads/atswitch/at-switch/total)](https://github.com/atswitch/at-switch/releases/latest)

### 🌐 Die einzige offizielle Website: **[atswitch.io](https://atswitch.io)**

[English](README.md) | [中文](README_ZH.md) | [日本語](README_JA.md) | Deutsch | [Changelog](CHANGELOG.md)

</div>

---

> [!WARNING]
>
> ## Erklärung zu offiziellen Kanälen (Bitte lesen)
>
> AT-Switch ist eine **vollständig kostenlose Open-Source-Desktop-Anwendung** und **erhebt keinerlei Gebühren**. Bitte beziehen Sie die Software ausschließlich über die folgenden offiziellen Kanäle:
>
> | Kategorie | Offizieller Link |
> | :--- | :--- |
> | **Offizielle Website** | **[atswitch.io](https://atswitch.io)** |
> | **Quellcode** | **[github.com/atswitch/at-switch](https://github.com/atswitch/at-switch)** |
> | **Releases** | **[GitHub Releases](https://github.com/atswitch/at-switch/releases)** |
> | **Fehlerberichte** | **[GitHub Issues](https://github.com/atswitch/at-switch/issues)** |
>
> Jede Website oder jeder Client, der unter dem Namen „AT-Switch“ Zahlungen oder persönliche Zugangsdaten verlangt, ist betrügerisch.

---

## Überblick

**AT-Switch** ist ein nativer Desktop-Manager für macOS und Windows, mit dem Entwickler Modell-Provider und LLM-Modelle in mehreren Coding-Agents mühelos konfigurieren und wechseln können.

Standardisierter Workflow: **Agent auswählen → Provider verwalten → Modell wählen → Sofort umschalten**.

- **Direktmodus als Standard**: Schreibt Konfigurationen direkt in die nativen Dateien der Agents, ohne Latenz oder Proxy-Overhead.
- **Lokaler Proxy bei Bedarf**: Für Protokollkonvertierungen (z. B. Codex Responses zu OpenAI Chat) oder API-Key-Isolierung lässt sich ein lokaler Proxy mit einem Klick zuschalten.
- **Höchste Sicherheit**: Entwickelt mit Tauri 2, Rust, React und TypeScript. Sensible API-Keys werden im Systemtresor (macOS Keychain / Windows Credential Manager) gesichert. Es werden niemals Prompts, Antworten oder Protokolldaten erfasst.

---

## ✨ Wichtigste Funktionen

- **Zentraler Provider-Katalog**: Verwalten Sie DeepSeek, Kimi, Zhipu GLM, Doubao, MiniMax, Qwen und benutzerdefinierte Endpunkte an einem Ort.
- **Unabhängige Agent-Profile**: Jeder Agent speichert seine eigenen Bindungen, Modelle und Modi.
- **Multi-Protokoll-Konvertierung**: Sichere Übersetzung zwischen **OpenAI Chat Completions**, **OpenAI Responses** und **Anthropic Messages**.
- **Streaming & Tool Calling**: Volle Unterstützung für SSE-Streaming und Function Calling.
- **Transaktionale Sicherheit**: Automatische verschlüsselte Sicherung vor Änderungen mit Validierung und Rollback bei Fehlern.
- **Prozess-Lifecycle**: Erkennt laufende Agent-Instanzen und startet sie bei Konfigurationsänderungen ordnungsgemäß neu.

---

## 💻 Unterstützte Plattformen & Download

Alle offiziellen Releases finden Sie unter [GitHub Releases](https://github.com/atswitch/at-switch/releases).

| Plattform | Mindestanforderung | Architektur | Paketformat |
| :--- | :--- | :--- | :--- |
| **macOS** | macOS 12 Monterey oder neuer | Apple Silicon / Intel / Universal | `.dmg` |
| **Windows** | Windows 10 / 11 | x64 | `.msi` / Portable (`.zip`) |

---

## 📄 Lizenz

Dieses Projekt ist unter der [MIT-Lizenz](LICENSE) lizenziert.
