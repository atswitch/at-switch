# 贡献指南 (Contributing to AT-Switch)

简体中文 | [English](./CONTRIBUTING_EN.md)

感谢关注并参与 AT-Switch 开源项目！我们欢迎所有形式的贡献，包括提出 Issue、完善文档、报告安全漏洞以及提交 Pull Request。

## 开发环境要求

- **Node.js**: 20+
- **npm**: 10+
- **Rust**: 1.85+ (stable)
- **操作系统**: macOS 12+ 或 Windows 10/11
- 参考 [Tauri 2 平台先决条件](https://v2.tauri.app/start/prerequisites/) 完成系统原生依赖配置。

## 本地开发流程

1. **Fork 并克隆仓库**：
   ```bash
   git clone https://github.com/<your-username>/at-switch.git
   cd at-switch
   ```

2. **安装前端依赖**：
   ```bash
   npm ci
   ```

3. **启动调试**：
   - 浏览器 Mock 模式：
     ```bash
     npm run dev
     ```
   - 本地桌面应用模式（需要本地 Rust 工具链）：
     ```bash
     npm run tauri:dev
     ```

## 代码质量门禁

在提交 Pull Request 之前，请务必在本地运行并通过所有门禁检查：

```bash
# 1. 前端类型检查与构建
npm run build

# 2. 前端单元测试
npm test -- --run

# 3. 依赖许可证元数据检查
npm run licenses:check

# 4. Rust 代码格式检查
cargo fmt --manifest-path src-tauri/Cargo.toml -- --check

# 5. Rust Clippy 代码质量检查
cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets -- -D warnings

# 6. Rust 核心单元测试
cargo test --manifest-path src-tauri/Cargo.toml
```

> **注意**：测试用例采用隔离的临时目录与内存数据库，绝不读取真实 API Key，也不会篡改真实 Agent 的配置文件。

## Pull Request 规范

1. **创建功能分支**：基于最新的 `main` 分支切出 `feat/<feature-name>` 或 `fix/<issue-number>`。
2. **聚焦单一职责**：保持每个 PR 范围聚焦，便于审查。
3. **跨平台兼容**：涉及 Agent 扫描发现与配置更新的改动，必须同时兼顾 macOS 与 Windows 平台逻辑。
4. **补充测试**：为新功能或 Bug 修复添加对应的单元测试。
5. **清晰的提交说明**：在 PR 描述中清晰说明改动动机、实现方案与验证步骤。
6. **同步变更记录**：用户可见行为变化应更新 [CHANGELOG.md](./CHANGELOG.md) 的
   `Unreleased` 部分。

## 发布维护

正式版本使用与四个工程版本文件一致的 `v*` 语义化版本标签触发。GitHub Actions
只创建 Draft Release；维护者必须完成签名、校验和真机安装验证后再人工公开。完整
检查项见 [.github/RELEASE_CHECKLIST.md](./.github/RELEASE_CHECKLIST.md)。

## 第三方素材

新增或更新第三方名称、图标和商标素材时，必须记录来源并确认允许随仓库分发，相关
说明同步更新到 [THIRD_PARTY_NOTICES.md](./THIRD_PARTY_NOTICES.md)。
