<div align="center">

# AT-Switch

### WorkBuddy、CodeBuddy、QClaw、AutoClaw、Codex、DuMate、Hermes、OpenCode、Kimi Work、AionClaw、ZCode のオールインワン管理・モデル切り替えツール

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

### サポート対象 Agent

| Agent | プラットフォーム | 状態 | ネイティブ・プロトコル | 備考 |
| --- | --- | --- | --- | --- |
| **WorkBuddy** | macOS / Windows | ✅ 対応 | OpenAI Chat Completions | 標準 XDG 設定の上書きを更新し、セッションフィールドと組み込みモデル別名を保持 |
| **CodeBuddy** | macOS / Windows | ✅ 対応 | OpenAI Chat Completions | JSONC コメント・末尾カンマ・組み込みアカウントを保持しつつ対象アカウントの標準 XDG 設定を更新 |
| **QClaw** | macOS / Windows | ✅ 対応 | OpenAI Chat Completions | プロセス状態を保持しつつ標準 XDG 設定を上書き |
| **AutoClaw** | macOS / Windows | ✅ 対応 | OpenAI Chat Completions | 標準 XDG 設定の選択アカウントを上書きし、未知フィールド・組み込み状態を保持 |
| **Codex** | macOS / Windows | ✅ 対応 | OpenAI Responses | `$CODEX_HOME/config.toml` または `~/.codex/config.toml` をきれいに更新 |
| **DuMate** | macOS / Windows | ✅ 対応 | OpenAI Chat Completions | すべてのセッションディレクトリと組み込みモデル別名に対して、選択中アカウントの永続 XDG オーバーライドを更新 |
| **Hermes Agent** | macOS / Windows | ✅ 対応 | OpenAI Chat Completions | `$HERMES_HOME/config.yaml` または `~/.hermes/config.yaml` を更新し、他の YAML フィールドを保持 |
| **OpenCode** | macOS / Windows | ✅ 対応 | OpenAI Chat Completions | 管理対象プロバイダとデフォルトモデルを更新し、JSONC コメント・末尾カンマ・サードパーティ製プロバイダを保持 |
| **ZCode** | macOS / Windows | ✅ 対応 | OpenAI Chat Completions / OpenAI Responses / Anthropic Messages | `~/.zcode/v2/provider_config.json` の管理対象プロバイダ・モデルルール・デフォルトモデルを更新し、他のプロバイダと未知フィールドを保持 |
| **Trae CN** | macOS / Windows | ✅ 対応 | OpenAI Chat Completions | Trae 内で設定済みのカスタムモデル間を切り替え（`state.vscdb` の選択記録を書き換え）。認証情報は Trae が暗号化管理し、AT-Switch は読み書きしない |
| **TRAE SOLO CN** | macOS / Windows | ✅ 対応 | OpenAI Chat Completions | 0.1.69 以降は選択中モデルが平文 `state.vscdb` に戻り、Trae CN と同様にアプリ内で設定済みのカスタムモデル間を切り替え可能。0.1.66 は暗号化 DB に保存していたため 0.1.69 以降が必要 |
| **QwenWork** | macOS / Windows | 🟡 検出のみ | — | カスタムモデルはクライアント側スイッチとサーバー側認可の二重ゲート。ローカルで解除できるのは前者のみで、モデルは選択できても呼び出しは 403 で拒否される（`You do not have access to this model service`）ため、設定は変更しない |
| **DoubaoWork** | macOS / Windows | 🟡 検出のみ | — | ユーザー階層の Provider / BYOK 設定項目が見つからないため、インストール状態のみ表示 |
| **Coze** | macOS / Windows | 🟡 検出のみ | — | ローカルデータは実行時およびログイン状態でありプロバイダ設定ではないため、インストール状態のみ表示 |
| **Kimi Work** | macOS / Windows | 🟡 検出のみ | — | Daimon は起動ごとに `model.current` をサーバー配信の既定モデルで上書きする（公式モデル名 `k3-agent` でも `k2d8-preview` に戻る）。そのため実行時 TOML の `default_model` は常に公式モデルとなり、独自プロバイダは TOML に現れても選択されない。書き込み経路がないため設定は変更しない |
| **AionClaw** | macOS / Windows | ✅ 対応 | OpenAI Chat Completions / OpenAI Responses / Anthropic Messages | サンドボックス内 `openclaw/state/openclaw.json` の管理対象プロバイダとデフォルトモデルを更新（QClaw と同一実装を共用） |
| **EasyClaw** | macOS / Windows | ✅ 対応 | OpenAI Chat Completions / OpenAI Responses / Anthropic Messages | 同じく OpenClaw カーネル（`gateway.asar/openclaw.mjs`）を内蔵。`~/.easyclaw/easyclaw.json` の管理プロバイダと `agents.defaults.model.primary` を更新。このファイルが権威設定（`EASYCLAW_CONFIG_DIR` が `~/.easyclaw` を指す）のため二重書き込みは不要 |
| **ima** | macOS / Windows | 🟡 検出のみ | — | ima にはカスタムモデルの入口があります（`Default/Preferences` の `kExtraSettingInfo`、ユーザー追加の `NMauto` を含む）が、**外部からの書き込みは保持されません**。ima 終了中に選択状態を書き換えても、起動 11 秒後に元の値へ書き戻され、当該モデル UUID はローカルの `Preferences` 以外に存在しないため、権威状態はサーバー側にあります。よって書き込みません |

---

## 📄 ライセンス

本プロジェクトは [MIT License](LICENSE) のもとで公開されています。
