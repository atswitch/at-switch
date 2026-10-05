# ima 接入机制与验收边界

本文记录 ima 适配所依据的客户端行为、配置事务要求和验收结果。客户端内部接口不是
腾讯对第三方承诺稳定的公开 API；下文的静态证据不能替代安装包与真实会话验收。

**2026-10-05 Windows 验收更新：** 本轮 Windows ima 接入验收全部通过：重新打包并
安装 `3.15.2`，验证原生凭据、两个入口的模型切换与真实问答、copilot 实际工具执行、
重复切换、运行中与未运行时的恢复，以及原有模型保留。已修复重启产生“ima 未正确
关闭”提示的问题，并验证正常退出。最终两个入口的远端首选和本地模型选择均恢复到
本轮开始时的状态。具体覆盖范围、测试结果及安装包标识见本文末尾的 Windows 实测记录。

**此前 macOS 验收记录：** macOS 真实账号已完成一次完整配置往返：原始模型 → GLM-5.2 →
重复切换 GLM-5.2 → 原始模型 → GLM-5.2 → 最终恢复。各阶段均回读两个场景的远端
首选和本地选择；重复切换未增加模型项，最终远端首选、本地选择和用户原有模型均恢复。
该次往返 38.64 秒完成。真实消息验收尚未完成：ima 的多桌面窗口无法由当前自动化环境
稳定定位，验收运行在首个原始模型阶段超时后已自动恢复。当时 Windows 仅完成官方包静态
研究、平台实现和独立模块编译检查，未进行系统凭据读取或切换恢复的真机验收。

最终仓库门禁结果应以本次变更完成后的命令输出为准；真实账号测试为显式 opt-in，
不进入普通 `cargo test`。调试包的 ad-hoc 签名不代表正式签名、公证或发布版本的权限
体验已经验收。

## 证据范围

- macOS 客户端 `Info.plist`：应用名称及可执行文件为 `ima.copilot`，Bundle Identifier
  为 `com.tencent.imamac`。标准安装目标为系统或当前用户的 `Applications` 目录下
  `ima.copilot.app`。
- ima 设置扩展 `5.6.3` 的模型管理客户端：`CustomizeModelsHTTPService`、
  `ModelManageHTTPService`、`CustomModelDialogPresenter`、`ModelTypePresenter`、
  `CopilotModelPresenter` 及两种模型缓存存储实现。
- 设置扩展可以独立更新。发现时应读取已安装扩展的 manifest，不把调研时的版本当成
  永久请求版本，也不能把遗留扩展缓存当作当前活动页面的代码。
- 配置机制静态调研不读取真实凭据、不修改 ima 配置。Windows 包通过腾讯官网公开
  下载渠道获取后静态检查，不执行安装器。账号接口及端到端结果在验收记录中另列。

## 配置源和两个场景

ima 自定义模型由 ima 自己的账号服务持久保存。设置扩展使用 `ima.qq.com` 的
`cgi-bin` 接口；AT-Switch 无需建立自己的云服务。

| 内容 | 来源 | 作用 |
| --- | --- | --- |
| 自定义模型配置 | `customize_models/get_homepage` | 返回自定义模型、厂商和容量默认值 |
| 问问 ima 的模型与首选 | `model_manage/get_models`，`scene: 0` | `preferred_model_id` 与可选模型列表 |
| 我的 copilot 的模型与首选 | `model_manage/get_models`，`scene: 1` | 独立的 `preferred_model_id` 与可选模型列表 |
| 当前设备模型选择 | 原生 `kExtraSettingInfo` | `modelConfig`、`copilotModelConfig` 和旧版 `modelType` |
| 当前页面选择 | 页面内存及 URL 参数 | 可能暂时优先于远端首选 |

`get_homepage` 的设置页消费字段是 `models`、`suppliers`、
`customize_model_config`。不能只读取该接口就声称已保存两个场景的原始首选。
必须分别读取两个 `get_models` 响应。

macOS 配置目录以系统 API 取得的用户 Application Support 目录为根，候选为
`com.tencent.imamac`。活跃账号来自 `Default/Preferences` 中的登录元数据，账号
凭据由系统凭据库解析。账号标识仅用于定位和隔离基线，不应显示、记日志或进入
测试 fixture。

本机只读结构检查确认：`Default/Preferences` 的顶层 `kExtraSettingInfo` 是一个
JSON 字符串，解析后包含 `modelConfig` 和 `copilotModelConfig` 对象。本地适配器
只管理这两个对象中的 `modelId`、`modelType`、`timestamp`；缺失对象可按原生合并
存储的行为创建。旧版根 `modelType` 不修改。

### 模型标识

`customize_id` 是新增、编辑、删除自定义模型时使用的标识；`model_id` 是场景选择
及首选接口使用的标识。必须分别保存并根据真实响应关联，不能通过模型名称或未经
验证的字符串拼接推导。

场景返回的模型还可能在 `sub_model_infos` 内包含可选的 `model_id` 和
`model_type`。ima 的查找逻辑会同时检查顶层与子模型。恢复思考模式等子选项时，
只搜索顶层模型会错误判定原选项已失效。

### 选择优先级

设置扩展初始化时按以下顺序选择有效模型：

1. 页面 URL 显式指定的模型；
2. 当前场景远端 `preferred_model_id`；
3. 本地缓存中有效的用户选择；
4. 服务端 `is_default` 模型。

远端首选为空且存在本地选择时，客户端可用 `set_if_absent: true` 将本地选择迁移
为远端首选。因此“远端无首选”和“远端显式选中当前默认项”具有不同语义。

Copilot 本地无选择时的静态缓存默认是 `modelType: 100000`、
`modelId: "official_100000"`。远端列表加载后的默认选择是首个 `is_default` 项，
没有该标记则取列表第一项。最终恢复依据必须来自当前有效列表，不能只写死本地
默认常量。自定义模型类型常量为 `1000000`。

## 接口形状

以下仅为客户端静态确认的字段示例，所有模型、地址与凭据均为虚构值。接口使用
POST JSON；客户端在 HTTP 边界把 camelCase 转为 snake_case。

```json
{
  "model_info": {
    "api_uri": "https://api.example.invalid/v1/chat/completions",
    "api_key": "example-secret-not-a-real-key",
    "model_name": "example-chat-model",
    "max_input_tokens": 32768,
    "max_output_tokens": 4096
  }
}
```

- `customize_models/add_model`：发送 `model_info`。自由填写接口的表单分支不发送
  预设厂商 ID。
- `customize_models/modify_model`：在同样的 `model_info` 中增加 `customize_id`。
- `customize_models/delete_model`：发送 `{"customize_id":"example-custom-id"}`。
- `customize_models/set_preferred_model`：发送
  `{"scene":0,"model_id":"example-selected-id","set_if_absent":false}`；
  场景 1 单独调用。
- `model_manage/get_models`：分别发送 `{"scene":0}`、`{"scene":1}`。

原生设置页在新增或修改成功后重新读取模型列表，而不是依赖新增响应推导选择 ID。
输入、输出上限以服务端的配置默认值或模型已知能力为准，不能把示例数值作为每个
Provider 的真实能力。

首版协议范围应限制到 ima 自定义接口实际支持的 OpenAI Chat Completions。
Provider 根地址必须复用 AT-Switch 的统一 Endpoint 解析，生成完整
`/v1/chat/completions`，不得重复拼接。不能仅凭成功保存配置声称模型支持工具调用、
图像、长上下文或已发生真实推理请求。

## 生效与用户体验

正常流程以用户已安装且登录 ima 为前提，在 AT-Switch 的现有模型列表中完成选择，
不要求用户手动复制登录 Cookie、运行脚本或编辑配置。

首次访问系统凭据可能触发系统授权。开发脚本访问成功不能证明正式签名安装包不会
弹窗或只弹一次；必须用正式身份的安装包验证，并把权限拒绝、账号退出、凭据过期
分别转成可执行的恢复说明。只在用户主动连接或切换时访问凭据，普通列表刷新不应
反复触发凭据提示。

**仅更新远端首选并激活 ima 窗口，不足以保证当前页面切换。** 静态代码中：

- 初始化、进入模型设置会执行 `sync-preferred`；
- 页面重新可见和打开模型菜单可执行 `refresh`；
- `refresh` 保留仍有效的当前选择，仅更新列表或对失效项回退；
- 两种缓存通过原生 `GetSettingByKey` / `SaveInfoWithKey` 访问
  `kExtraSettingInfo`，读取后合并保存。

在没有验证稳定的外部刷新入口之前，可靠生效必须结合现有生命周期管理：保存运行
状态、安全退出、执行配置事务、校验后按原状态重启。不应修改运行中会被 ima 覆盖
的文件，不修改安装包，不修改每个会话的配置。正在回答的旧会话与新会话必须分别
验收，不能靠测试请求显式传模型绕过默认选择。

## 管理字段和恢复事务

接管前基线至少包括：两个场景的首选值及其是否存在、原始本地受控字段及是否存在、
当前账号的稳定隔离标识，以及 AT-Switch 已管理模型的服务端标识。基线保存在现有
受保护的配置事务中，首次成功接管后不可被后续切换覆盖。

| 字段或内容 | 处理原则 |
| --- | --- |
| AT-Switch 新建模型项 | 用保存的服务端标识管理，不通过名称认领已有项 |
| 原有自定义模型与 API Key | 不覆盖、不删除；不存入明文数据库或日志 |
| 两个场景远端首选 | 精确备份、分别写入、分别回读、失败补偿 |
| 本地模型选择字段 | 仅修改已证明必要的字段，保留未知键与增强设置 |
| 登录态、会话、知识库和其他设置 | 不修改 |
| 切换后用户新增的无关内容 | 恢复时保留，不整文件覆盖 |

本地与远端不存在共同的原子事务，必须记录可恢复操作状态：先备份并检查可恢复性，
再写入受管理项及首选，随后回读校验，最后提交绑定状态。远端超时是结果不确定，
应先回读确认再重试；不能直接再次新增，导致重复模型。第二个场景失败时需要恢复
第一个场景，不能把半完成状态显示为已应用。

当前恢复流程先从首次接管时的本地选择和当前有效模型列表解析两个场景的恢复目标，
恢复远端首选并回读，再恢复本地受控字段。ima 服务会拒绝“删除后立即用相同参数重建”
（业务码 `100006`），因此 AT-Switch 创建的模型项在恢复后保留一条但不再选中；下次
切换复用并更新该项，避免重复模型和服务端频控。用户原有自定义模型始终保留。

**无首选恢复：** 客户端没有清除远端首选的调用；两个场景发送空 `model_id` 均返回
业务码 `51`。真实账号的接管前远端首选为空，因此恢复时优先使用首次接管时保存的
本地模型 ID；若它已不在当前列表中，则使用当前服务端默认模型的首个可选子模型，
没有默认标记时使用列表中的首个可选模型。恢复后必须回读确认目标有效，不能留下
指向已删除模型的悬空 ID。

受控验证已确认：两个场景的 `set_preferred_model` 发送空 `model_id` 均返回
业务码 `51`，不能用该操作实现清除。客户端删除模型后仅刷新列表，不额外调用
首选设置接口；删除受管理项时服务端如何处理引用它的首选仍须单独回读验证。

账号变化时必须拒绝将另一个账号的基线应用到当前账号。单次切换期间发生的用户
并发编辑也需要检测，不能覆盖。

## 平台范围与验收记录

macOS 的应用身份与设置扩展行为已有上述静态证据。

Windows 来源为[腾讯 ima 官方下载页](https://ima.qq.com/download)的公开下载配置。
调研时官网正式渠道返回的 `2.6.9_5083` 安装器可下载于腾讯的
`app-dl.ima.qq.com` 域名；以下通过安装器及嵌入客户端的字符串、PE 导入和相关
函数控制流确认，未执行安装器：

| 项目 | Windows 正式渠道位置或行为 |
| --- | --- |
| 默认可执行文件 | `%LOCALAPPDATA%\ima.copilot\Application\ima.copilot.exe` |
| 用户数据根 | `%LOCALAPPDATA%\ima.copilot\User Data` |
| 资料配置 | 用户数据根下 `Default\Preferences` |
| 安装注册表项 | `SOFTWARE\Ima.copilot`、`SOFTWARE\Tencent\ima.copilot` |
| 自定义安装路径值名 | `ImaInstallPath` |
| 当前账号元数据 | `tencent.wxlogin.account_meta` |
| 受系统保护的账号凭据 | `tencent.wxlogin.account_secret_encrypted` |

默认安装根来自安装器的 `SHGetFolderPathW(CSIDL_LOCAL_APPDATA)` 调用后拼接
`ima.copilot\Application`。用户数据根来自客户端读取 Windows 路径键 `112`
后拼接 `ima.copilot`（可含渠道后缀）和 `User Data`；路径键 `112` 对应
`DIR_LOCAL_APP_DATA`，参见 [Chromium Windows 路径定义](https://chromium.googlesource.com/chromium/src/+/main/base/base_paths_win.h)。
运行时应使用系统目录 API 和现有发现抽象，而不是展开环境变量后写死路径。

Windows `account_secret_store_win.cc` 的加载分支读取上述凭据字段为字符串，执行
Base64 解码，再调用 `CryptUnprotectData`。已确认相关调用的 description、entropy、
reserved、prompt 参数为空，flags 为 `0`，返回内存通过 `LocalFree` 释放。
`credential_id` 没有作为 DPAPI 的额外 entropy；它用于客户端日志关联。AT-Switch
不得复制该日志行为输出标识或凭据。

`ima/windows_auth.rs` 已实现该 DPAPI 读取分支：从指定活跃资料的配置进行有界读取，
只在内存中解码并验证凭据结构，以 `SecretValue` 返回；输入、解码结果、明文副本和
系统返回缓冲区均在释放前清零。错误仅使用稳定错误码，不包含原始配置或 OS 输出。

旧版 `v10` OSCrypt 格式另有原生迁移分支，读取 `Local State` 的
`os_crypt.encrypted_key` 并在解密成功后重存为 DPAPI。首版可明确要求遇到旧格式
的用户先打开新版 ima 完成原生迁移，不自行修改登录数据或降低系统保护。

以上是此前静态研究的证据范围，本身不能替代 Windows 真机切换、恢复或安装包验收。
两个平台共享领域流程、错误与事务；本轮 Windows 的真实凭据解密、网络鉴权、
安装与重启验证已完成，结果见末尾记录。

本地缓存候选生成、恢复和校验已有 10 个纯文件单元测试，覆盖嵌套 JSON 字符串、
缺失容器、未知字段保留、后续用户编辑、原值与 null/缺失精确恢复、重复切换、
敏感值不进入受控快照、坏格式错误以及远端补偿重新生成模型 ID 时的映射。测试
使用临时目录和虚构数据；仓库中 `cargo test --manifest-path src-tauri/Cargo.toml
agents::ima_local --lib` 的 10 项测试已通过，并已纳入本轮完整门禁。

Windows 认证另有 5 项纯解析测试，覆盖字段读取、非法 Base64、旧版迁移提示、大小
边界、凭据类型和空 token 校验，已在仓库测试中通过。实际 Windows 认证源文件在
独立轻量测试工程中通过 `x86_64-pc-windows-msvc` 的类型检查和 Clippy；该检查不
包括完整 Tauri 链接、安装包或 Windows 系统上的 DPAPI 调用。

安装发现与入口验证的 7 项测试已在 macOS 上通过，覆盖标准安装及自定义改名应用、
缺失安装、未登录或缺失配置、设置扩展的数字版本排序，以及公网 Endpoint 限制。
测试使用临时目录与虚构账号字段，不读取系统凭据、不启动 ima。Windows 标准目录、
自定义安装和用户目录回退另有平台限定 fixture，本轮已在本机 Windows 完整测试中
执行；未额外触发远端 GitHub Actions。

验收覆盖与后续发布边界如下；真机验证与纯测试分别标明：

- [x] 真实接口模型 ID 关联、两个场景首选读取与恢复。
- [x] 原远端首选缺失时，从接管前本地选择或当前有效默认项恢复并回读。
- [x] 原始模型 → 第三方 GLM-5.2 → 重复切换 → 原始模型 → GLM-5.2 → 最终恢复。
- [x] 重复切换不增加多余模型项，原有用户自定义模型保持不变。
- [x] 安全退出与重启后两个入口的远端首选和本地默认选择一致。
- [x] 两个入口新建真实会话，不显式指定模型，由默认配置选中 GLM-5.2 和
  DeepSeek V4 Flash 并成功回答；copilot 中实际执行工具并返回正确结果。
- [x] 远端失败、本地失败、结果不确定及恢复补偿路径的纯测试通过；未向真实账号
  注入破坏性故障。
- [x] Windows 安装升级、发现、凭据、切换、恢复和正常重启真机验证。
- [x] ima 未运行时切换、恢复均保持未运行状态。
- [ ] 上游物理模型身份与请求发起方位置的独立审计；当前只声明配置与客户端执行证据。
- [ ] 正式签名 macOS 安装包首次/重复授权、重启、登录失效及切换账号。
- [ ] 远端平台 CI、安装向导交互、卸载及自动更新器流程；本轮验证的是本机静默升级。
- [x] 原有 Agent 回归与仓库要求的构建、测试、格式和 lint 门禁。

在真实请求发起方未得到实证前，不宣称 AT-Switch 本地代理可供 ima 使用。
如验证由 ima 远端访问 Provider，则 localhost、私有网络和 AT-Switch 本机监听地址
不能直接作为这条路线的 Provider 地址；也不能为隐藏此限制而提供未授权的隧道。

## Windows 实测记录（2026-10-04 至 2026-10-05）

### 环境与构建

- 分支：`release/ima_op`；基准提交：`3816909a79ccfd2a7ebb91ce03be8a036d5ede37`。
- AT-Switch：`3.15.2`，包含本轮尚未提交的验证修复。
- Windows x64；ima 首轮识别为 `2.6.12.5194`，后续识别为 `2.6.12.5195`。
- 用户已确认 Mac 验证通过；本轮没有独立复测 Mac。
- 重新执行 `node scripts/tauri-build.mjs --target x86_64-pc-windows-msvc --bundles nsis`。
- 编译后的首轮打包遇到文件占用，待编译结束后执行
  `node node_modules/@tauri-apps/cli/tauri.js bundle --target x86_64-pc-windows-msvc --bundles nsis`；
  最终打包成功，未再出现 bundler 标记写入失败。
- 用最终 NSIS 包的 `/S` 静默安装完成 `3.15.1` → `3.15.2` 升级，退出码为 `0`。
  安装位置为 `D:\develop\environment\at-switch\at-switch.exe`；卸载注册表、程序版本、
  实际运行路径与包内主程序哈希均已核对。最终 DeepSeek copilot 问答和工具执行使用
  该安装位置启动的程序完成切换。安装向导交互与卸载流程不在本轮覆盖范围。
- 安装包：`src-tauri/target/x86_64-pc-windows-msvc/release/bundle/nsis/AT-Switch_3.15.2_x64-setup.exe`。
- 安装包大小：`4062481` 字节。
- 安装包 SHA-256：`9B5E9DAB024CE16F182024DCA445B17A884D7C3D09DF4E791BF5BAB62ECA732C`。
- 包内及已安装主程序 SHA-256：`573399A7E37A2C5B93A155DB3178ACE492786E906D402E141D4F62610DCC2326`。
- 主程序 PE 架构为 x64（`8664`）；NSIS 外层引导程序为 x86（`14c`），属于正常封装。
  包内程序与打包前构建产物仅有 Tauri bundle 类型标记的 3 字节差异（`UNK` → `NSS`）；
  安装后的程序与包内程序逐字节一致。Windows 按现有规范不签名。
- 所有产品修复均已包含在最终包中；打包后进一步调整的验收断言位于 `cfg(test)`，
  不影响产品二进制。本轮没有发布、合并或推送。

### 已通过

| 检查 | 结果与证据 |
| --- | --- |
| 前端构建与测试 | `npm run build` 成功；14 个测试文件、107 项测试通过 |
| Windows Rust 测试 | 187 项通过，0 失败，5 项显式忽略；真实账号和正常重启测试按授权另行执行 |
| 静态检查与格式 | `cargo clippy --all-targets --locked --offline -- -D warnings`、`cargo fmt --all -- --check`、`git diff --check` 通过 |
| 依赖许可 | 295 个 npm 包、546 个 Rust crate 的许可检查通过 |
| 安装与发现 | 静默升级至 3.15.2 成功；新包自动识别 ima、版本、已登录账号和运行状态；全 Agent 发现回归通过 |
| 首次连接取消 | 取消确认后保留原模型，未执行切换 |
| Windows 原生凭据 | 通过当前 Windows 用户的 DPAPI 读取 ima 凭据，实际账号接口读取成功；没有输出 token/API Key |
| 两个场景切换 | GLM-5.2、deepseek-v4-flash 的远端首选及本地选择均指向管理模型，事务 `pending=false`；重启后两个入口均按默认选择使用目标模型 |
| GLM-5.2 真实请求 | 新建「问问 ima」会话，未手动指定模型；`QA-GLM-01` 的 `17+25` 返回 `42` |
| DeepSeek 真实请求 | 新建「问问 ima」会话，未手动指定模型；`QA-DS-01` 的 `29+13` 返回 `42` |
| GLM-5.2 copilot | `CP-GLM-01` 的 `31+11` 返回 `42`；工具测试实际执行 `python3 -c "print(12345*6789)"`，执行卡片及回答均为 `83810205` |
| DeepSeek copilot | 最终安装包切换后，`CP-DS-01` 的 `19+23` 返回 `42`；实际执行 `python3 -c "print(23456*789)"`，执行卡片及回答均为 `18506784` |
| 正常退出重启 | Windows Restart Manager 请求正常退出；ima 自身写入正常退出状态；重启后不再新增“ima 未正确关闭”提示，多次切换与恢复均通过 |
| 配置往返 | 原配置 → GLM-5.2 → 重复 GLM-5.2 → 原配置 → GLM-5.2，并增加未运行时恢复与切换，最终恢复；最终真实账号测试 66.14 秒通过 |
| 未运行状态保留 | ima 停止时恢复与切换均断言 `NotRunning`，不额外启动应用；验收结束恢复原运行状态 |
| 重复与保留 | 重复切换没有额外模型行；配置往返前后的原有用户模型和两场景选择均一致 |
| 最终恢复 | 新包执行恢复后，两场景远端基线关系均为 `exact`，`managed_selected=false`、`pending=false`；本地两入口 modelId/modelType 与本轮开始时一致 |

用户已明确授权将「蒙云智算」的接口地址、API Key 和上述模型名保存到当前腾讯 ima
账号，允许重启和发送少量无个人信息的测试问题，并要求结束后恢复选择；随后授权
完成 copilot 创建和安装验证。已创建「AT-Switch 验证」copilot，按授权确认其创建条款。
最终两个入口均恢复原来的 `NMauto` 选择；远端基线精确匹配，本地 modelId/modelType
与本轮开始时一致。按现有设计，AT-Switch 创建的模型行保留为未选中，供后续复用；
没有删除用户原有模型。验证用 copilot 和四条无个人信息的测试会话保留在 ima 中。

上述真实消息证据由所选模型标签、远端首选、本地配置和实际回答共同组成，未取得
供应商侧请求审计记录，因此仍不宣称已独立证明上游物理模型或请求发起方位置。

### 本轮保留的修复

1. Windows 全 Agent 安装发现测试增加 ima 标准安装路径，注册数量从 6 更新为 7。
2. 将显式 opt-in 的真实账号验收与脱敏诊断入口扩展到 Windows，修复 Windows
   `NativeSecretStore` 单元结构体构造引起的 Clippy 错误。
3. 连接确认文案区分 macOS Keychain 与 Windows 当前用户加密凭据。
4. Windows 重启 Agent 时断开子进程的标准输入/输出/错误继承，避免 ima 自身调试输出
   混入 AT-Switch 或验收日志。本轮既有混合日志已仅保留预定义验收结论。
5. ima 在 Windows 上使用 Restart Manager 正常退出，替代通用进程树结束方式。
   先按完整可执行路径定位唯一主进程，排除 renderer 等辅助进程，再核对路径与进程
   创建时间；仅注册该进程并调用 `RmShutdown`，flags 为 `0`，不使用强制结束。
   如果客户端拒绝退出，则返回错误并提示从托盘退出后重试，不继续改写配置。
   本轮没有通过修改崩溃标记或隐藏恢复提示绕过问题。
6. 完善真实账号验收断言：只允许由受保护检查点证明属于 AT-Switch 的一条管理模型
   保留并复用，原有用户模型仍逐项比较；新增纯测试确保用户行变化或额外行仍会失败。
   ima 重启后会刷新选择时间戳，运行态比较保留对象存在性、modelId 和 modelType，
   排除客户端自行刷新的 timestamp；产品恢复事务写入时仍精确校验受控快照。

Restart Manager 的退出语义见 Microsoft 的
[RmShutdown 文档](https://learn.microsoft.com/en-us/windows/win32/api/restartmanager/nf-restartmanager-rmshutdown)。

### 验收边界与证据

本轮 Windows ima 接入范围已通过，不再有等待授权的项目。真实会话覆盖当前账号、
「蒙云智算」公网 OpenAI Chat Completions 接口及上述两个模型；没有据此声称所有
Provider、图像输入、长上下文或上游物理模型均已验证，也不支持据此将 localhost
作为 ima 的 Provider 地址。

退出登录、换账号、凭据轮换、远端或本地提交失败、结果不确定、重复新增恢复和
并发互斥由纯测试覆盖。本轮未退出用户账号、登录第二个真实账号或向账号注入故障。
未独立复测 Mac，也未验证 GUI 安装向导、卸载、自动更新器或触发远端 CI。

日志保存在工作区 `../.release-work/ima-windows-20261004/`，重点文件为
`frontend-tests.log`、`rust-tests-completed.log`、`rust-clippy-completed.log`、
`licenses-final.log`、`windows-build-verified.log`、`windows-bundle-final.log`、
`package-verification.json`、`ima-restart-manager-confirmed.log`、
`ima-roundtrip-final-v2.log`、`ima-installed-deepseek-checkpoint.log`、
`ima-completed-restoration.log` 和 `local-restoration-completed.json`。
早期失败实验保留作排查历史，最终结论以以上完成后的记录为准。实际问答和工具执行
通过原生窗口的可访问性内容与截图核对。不将账号凭据、完整 Preferences 或加密备份
提交到 Git。
