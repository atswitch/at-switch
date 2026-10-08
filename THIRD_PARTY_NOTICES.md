# Third-Party Notices and Trademarks

AT-Switch integrates with third-party AI agents and model providers. Product names,
logos, and trademarks are used only to identify compatible products and services.
Their inclusion does not imply endorsement, sponsorship, or affiliation.

Tencent ima / ima.copilot is identified by its product name for compatibility with
its model settings. The official product source is [Tencent ima](https://ima.qq.com/).
AT-Switch uses the official website favicon solely to identify this compatible
product. The source asset is published by Tencent at
`https://fe-static.ima.myqcloud.com/ima/assets/chat/favicon.svg`; the bundled
`src/assets/agents/ima.png` is a 128×128 PNG conversion. The product name, icon,
and trademarks remain the property of their respective owners and are not
relicensed under this repository's MIT License.

## Bundled Identification Assets

The repository contains the following identification assets. Each row records
the asset's source category and distribution basis.

| Product or service | Files | Source category | Distribution record |
| --- | --- | --- | --- |
| WorkBuddy | `src/assets/agents/workbuddy.png` | Official macOS application icon | Maintainer-confirmed permission |
| CodeBuddy CN | `src/assets/agents/codebuddy.png` | Official macOS application icon | Maintainer-confirmed permission |
| QClaw | `src/assets/agents/qclaw.png` | Official macOS application icon | Maintainer-confirmed permission |
| AutoClaw | `src/assets/agents/autoclaw.png` | Official macOS application icon | Maintainer-confirmed permission |
| OpenAI Codex | `src/assets/agents/codex.png` | Official application icon | Maintainer-confirmed permission |
| Baidu DuMate | `src/assets/agents/dumate.png` | Official macOS application icon | Compatibility identification; upstream trademark terms apply |
| Tencent ima | `src/assets/agents/ima.png` | Official website favicon | Compatibility identification; upstream trademark terms apply |
| TRAE TraeCode | `src/assets/agents/traecode.svg` | Vector trace of the official website favicon at `https://lf-static.traecdn.us/obj/trae-ai-tx/trae_website/favicon.png` | Compatibility identification; upstream trademark terms apply |
| TRAE TraeWork | `src/assets/agents/traework.svg` | Vector trace of the official Web App icon at `https://work.trae.ai/icon-192.png` | Compatibility identification; upstream trademark terms apply |
| DeepSeek | `src/assets/providers/deepseek.png`, `src/assets/providers/deepseek.ico` | Maintainer-provided product asset | Maintainer-confirmed permission |
| Doubao | `src/assets/providers/doubao.png` | Maintainer-provided product asset | Maintainer-confirmed permission |
| Kimi / Moonshot AI | `src/assets/providers/kimi.png`, `src/assets/providers/kimi.ico` | Maintainer-provided product asset | Maintainer-confirmed permission |
| MiniMax | `src/assets/providers/minimax.png`, `src/assets/providers/minimax.ico` | Maintainer-provided product asset | Maintainer-confirmed permission |
| Mongyun | `src/assets/providers/mongyun.png` | Maintainer-provided product asset | Maintainer-confirmed permission |
| Qwen | `src/assets/providers/qwen.svg` | Maintainer-provided product asset | Maintainer-confirmed permission |
| Zhipu AI | `src/assets/providers/zhipu.svg` | Maintainer-provided product asset | Maintainer-confirmed permission |

These names, logos, and trademarks remain the property of their respective owners.
They are not relicensed under the repository's MIT License. Redistribution and use
must continue to comply with the relevant owners' trademark and brand policies.
Maintainers must retain the underlying permission or provenance evidence, even when
that evidence cannot be published in this repository, and re-check it whenever an
asset is replaced.

## Software Dependencies

AT-Switch depends on open-source JavaScript and Rust packages. Exact package names,
versions, and registry checksums are pinned by `package-lock.json` and
`src-tauri/Cargo.lock`; license terms come from each upstream package rather than
from `Cargo.lock`. Each dependency remains subject to its own license. Release
maintainers must review the resolved dependency licenses and bundle any notices or
license texts required by those terms.

## Contributor Covenant

`CODE_OF_CONDUCT.md` is adapted from Contributor Covenant version 2.1, as stated in
that file.
