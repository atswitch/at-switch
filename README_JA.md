<div align="center">

# AT-Switch

### WorkBuddy、CodeBuddy、QClaw、AutoClaw、Codex のオールインワン管理・モデル切り替えツール

[![Version](https://img.shields.io/github/v/release/atswitch/at-switch?color=blue&label=version)](https://github.com/atswitch/at-switch/releases)
[![Platform](https://img.shields.io/badge/platform-macOS%20%7C%20Windows-lightgrey.svg)](https://github.com/atswitch/at-switch/releases)
[![Built with Tauri](https://img.shields.io/badge/built%20with-Tauri%202-orange.svg)](https://tauri.app/)
[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](https://opensource.org/licenses/MIT)
[![Downloads](https://img.shields.io/github/downloads/atswitch/at-switch/total)](https://github.com/atswitch/at-switch/releases/latest)

### 🌐 唯一の公式サイト：**[atswitch.io](https://atswitch.io)**

[中文](README.md) | [English](README_EN.md) | 日本語 | [العربية](README_AR.md) | [Changelog](CHANGELOG.md)

</div>

---

> [!WARNING]
>
> ## 公式チャンネルに関する声明（必ずお読みください）
>
> AT-Switch は**完全無料のオープンソース**デスクトップアプリケーションであり、**料金を請求することは一切ありません**。必ず以下の公式チャンネルからのみ入手してください：
>
> | カテゴリ | 公式リンク |
> | :--- | :--- |
> | **公式サイト** | **[atswitch.io](https://atswitch.io)** |
> | **ソースコード** | **[github.com/atswitch/at-switch](https://github.com/atswitch/at-switch)** |
> | **ダウンロード** | **[GitHub Releases](https://github.com/atswitch/at-switch/releases)** |
> | **フィードバック** | **[GitHub Issues](https://github.com/atswitch/at-switch/issues)** |
>
> 「AT-Switch」を名乗り、料金の支払いやチャージ、個人認証情報を要求するサイトやアプリはすべて詐欺です。

---

## 概要

**AT-Switch** は、macOS および Windows 向けのネイティブデスクトップ管理ツールです。複数の AI コーディング Agent におけるモデルプロバイダーやモデルの切り替えを、直感的かつ迅速に行うことができます。

Agent ごとに散らばった設定ファイルを探す必要はありません：**Agent を選択 → プロバイダーを登録 → モデルを選択 → ワンクリックで切り替え**。

- **ダイレクトモード優先**：標準では各 Agent のネイティブ設定ファイルを直接書き換えるため、プロキシによるレイテンシや負荷が発生しません。
- **ローカルプロキシ対応**：プロトコル変換（Codex Responses と Chat プロトコル間の変換など）や API キーの隔離が必要な場合は、内蔵ローカルプロキシを簡単に有効化できます。
- **安心のローカルセキュリティ**：Tauri 2、Rust、React、TypeScript で開発されています。API キーは OS の安全な資格情報ストア（macOS Keychain / Windows Credential Manager）に保存され、ユーザープロンプトやログを収集することはありません。

---

## ✨ 主な機能

- **プロバイダーの一元管理**：DeepSeek、Kimi、Zhipu GLM、Doubao、MiniMax、Qwen などの各種 LLM およびカスタムエンドポイントをまとめて管理。
- **Agent ごとの独立設定**：Agent ごとにバインドされたモデルや接続モードを個別に保持。
- **プロトコル相互変換**：**OpenAI Chat Completions**、**OpenAI Responses**、**Anthropic Messages** 間での安全な変換に対応。
- **ストリーミング & ツール呼び出し**：高度な内蔵コーデックにより、SSE ストリーミングと Function Calling を完全サポート。
- **トランザクション保護 & ロールバック**：設定書き換え前に暗号化バックアップを作成し、書き込み検証と自動ロールバックを実行。
- **プロセスの自動検知と再起動**：起動中の Agent を自動検知し、設定切り替え時に安全に再起動。

---

## 💻 対応プラットフォームとダウンロード

公式インストーラーはすべて [GitHub Releases](https://github.com/atswitch/at-switch/releases) で配布されています。

| プラットフォーム | 推奨 OS | アーキテクチャ | パッケージ形式 |
| :--- | :--- | :--- | :--- |
| **macOS** | macOS 12 Monterey 以降 | Apple Silicon / Intel / Universal | `.dmg` |
| **Windows** | Windows 10 / 11 | x64 | `.msi` / ポータブル版 (`.zip`) |

---

## 📄 ライセンス

本プロジェクトは [MIT License](LICENSE) のもとで公開されています。
