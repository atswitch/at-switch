# ima 接入机制与验收边界

本文记录 ima 适配所依据的客户端行为、配置事务要求和待验收项。客户端内部接口不是
腾讯对第三方承诺稳定的公开 API；下文的静态证据不能替代安装包与真实会话验收。

**当前验收状态：** macOS 真实账号已完成一次完整配置往返：原始模型 → GLM-5.2 →
重复切换 GLM-5.2 → 原始模型 → GLM-5.2 → 最终恢复。各阶段均回读两个场景的远端
首选和本地选择；重复切换未增加模型项，最终远端首选、本地选择和用户原有模型均恢复。
该次往返 38.64 秒完成。真实消息验收尚未完成：ima 的多桌面窗口无法由当前自动化环境
稳定定位，验收运行在首个原始模型阶段超时后已自动恢复。Windows 仅完成官方包静态
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

这些事实可以支持 Windows 发现与原生凭据读取实现，但不能当作 Windows 真机
切换、恢复或安装包授权验收。两个平台共享领域流程、错误与事务；真实凭据解密、
网络鉴权和重启验收仍需对应平台完成。

本地缓存候选生成、恢复和校验已有 10 个纯文件单元测试，覆盖嵌套 JSON 字符串、
缺失容器、未知字段保留、后续用户编辑、原值与 null/缺失精确恢复、重复切换、
敏感值不进入受控快照、坏格式错误以及远端补偿重新生成模型 ID 时的映射。测试
使用临时目录和虚构数据；仓库中 `cargo test --manifest-path src-tauri/Cargo.toml
agents::ima_local --lib` 的 10 项测试已通过，仍需最终完整门禁验证。

Windows 认证另有 5 项纯解析测试，覆盖字段读取、非法 Base64、旧版迁移提示、大小
边界、凭据类型和空 token 校验，已在仓库测试中通过。实际 Windows 认证源文件在
独立轻量测试工程中通过 `x86_64-pc-windows-msvc` 的类型检查和 Clippy；该检查不
包括完整 Tauri 链接、安装包或 Windows 系统上的 DPAPI 调用。

安装发现与入口验证的 7 项测试已在 macOS 上通过，覆盖标准安装及自定义改名应用、
缺失安装、未登录或缺失配置、设置扩展的数字版本排序，以及公网 Endpoint 限制。
测试使用临时目录与虚构账号字段，不读取系统凭据、不启动 ima。Windows 标准目录、
自定义安装和用户目录回退另有平台限定 fixture，仍待 Windows CI 执行。

下列项目必须由主开发流程补充实测结果后，才可声称正式功能完成：

- [x] 真实接口模型 ID 关联、两个场景首选读取与恢复。
- [x] 原远端首选缺失时，从接管前本地选择或当前有效默认项恢复并回读。
- [x] 原始模型 → 第三方 GLM-5.2 → 重复切换 → 原始模型 → GLM-5.2 → 最终恢复。
- [x] 重复切换不增加多余模型项，原有用户自定义模型保持不变。
- [x] 安全退出与重启后两个入口的远端首选和本地默认选择一致。
- [ ] 新建真实会话，不显式指定模型，由默认配置选中目标；从 ima 或上游证据
  确认模型、Endpoint 和请求发起方。
- [ ] 远端失败、本地失败、超时结果不确定、删除失败均保持可恢复状态。
- [ ] 正式签名 macOS 安装包首次/重复授权、重启、登录失效及切换账号。
- [ ] Windows 安装/发现/凭据/切换/恢复/重启真机验收及平台 CI。
- [ ] 原有 Agent 回归与仓库要求的构建、测试、格式和 lint 门禁。

在真实请求发起方未得到实证前，不宣称 AT-Switch 本地代理可供 ima 使用。
如验证由 ima 远端访问 Provider，则 localhost、私有网络和 AT-Switch 本机监听地址
不能直接作为这条路线的 Provider 地址；也不能为隐藏此限制而提供未授权的隧道。
