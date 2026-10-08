# TraeCode / TraeWork 直连接入与验收记录

本文记录 AT-Switch 对 TraeCode 和 TraeWork 的接入依据、受控范围、失败恢复与当前验收
边界。实现只使用 Trae 官方自定义模型界面，不修改安装包、私有数据库、会话目录或
启动参数，也不让模型请求经过 AT-Switch。

## 产品身份与发现

2026-10-08 在 macOS 实机确认：

| AT-Switch ID | 显示名 | macOS 应用 | Bundle Identifier | 已验版本 | 用户数据目录 |
| --- | --- | --- | --- | --- | --- |
| `traecode` | TraeCode | `Trae CN.app` | `cn.trae.app` | `3.3.104` | `Application Support/Trae CN` |
| `traework` | TraeWork | `TRAE SOLO CN.app` | `cn.trae.solo.app` | `0.1.69` | `Application Support/TRAE SOLO CN` |

Windows 发现覆盖当前用户标准安装目录、自定义安装目录、注册表 App Paths 和运行进程
路径；标准可执行文件候选为 `Trae CN.exe` 和 `TRAE SOLO CN.exe`。Windows 路径和
UI Automation 实现已纳入平台代码与测试输入，但本轮没有 Windows 真机，因此不得把
macOS 上的结果表述为 Windows 真机通过。

两个产品的活跃账号从 `User/globalStorage/state.vscdb` 中模型列表缓存键的账号前缀
识别。数据库只以 SQLite 只读方式用于账号隔离和模型项回读校验；它不是写入目标。
模型设置的权威持久化由 Trae 自己的设置界面完成。

## 为什么不直接改配置文件

实机检查确认 `state.vscdb` 中的模型列表是运行缓存，不是稳定的公开配置契约；真正
的自定义模型写入由应用内部服务处理，相关数据还有加密和运行时同步。直接改数据库
或调用私有 IPC 容易被启动过程覆盖，也无法可靠执行 Trae 自带的连通性测试。

因此本接入采用 Trae 官方 UI：

- macOS 使用系统 Accessibility API，并为 Electron 设置进程级
  `AXManualAccessibility`；
- Windows 使用系统 UI Automation；请求 JSON 通过 PowerShell 标准输入传递，API Key
  不进入命令行；
- 仅在用户点击切换或恢复并确认后操作界面；状态扫描不主动打开应用；
- Trae 未运行时返回可恢复错误，不由 AT-Switch 主动启动；
- macOS 首次使用需要用户授予 AT-Switch 辅助功能权限。

## 协议、Endpoint 与模式

TraeCode 与 TraeWork 官方表单均提供以下三种格式：

- OpenAI Chat Completions；
- OpenAI Responses API；
- Anthropic Messages。

AT-Switch 只允许 **Direct**，并要求源协议与 Provider 原生协议一致。Provider 保存的
地址仍是统一的 Base URL：若 Trae 表单要求完整请求 URL，则复用公共 Endpoint 解析器
分别生成 `/v1/chat/completions`、`/v1/responses` 或 `/v1/messages`；表单要求 Base
URL 时写入根地址。已包含协议路径的地址不会重复拼接。

模型请求由 Trae 直接发往 Provider。Trae 不接入 AT-Switch 本地代理，其他 Agent 的
Direct/Proxy 能力和默认行为均未改变。

## 配置事务、Secret 与恢复

首次接管前，AT-Switch 通过现有 `ConfigTransaction` 写入并回读验证加密检查点，内容
包括当前账号范围、接管前模型选择、AT-Switch 创建的模型显示名、当前受管项和未完成
状态。检查点只保存 API Key 的 SHA-256，用于判断是否可以复用已有模型项；明文 API
Key 只从系统凭据库进入本次官方表单操作，内存副本在销毁时清零，不进入 SQLite、日志
或界面状态。

切换顺序为：

1. 校验 Direct 模式、原生协议和当前账号；
2. 加密保存可恢复检查点、精确清理目标并标记操作中；
3. 通过官方表单新增模型并等待 Trae 连通性测试成功；旧版同模型 ID 项会先按受管标记
   精确删除，任一后续步骤失败则重建旧项并恢复原选择；
4. 在主模型选择器选择该项并重新回读；
5. 最后提交 AT-Switch 绑定记录，再清除操作中标记。

清理目标在任何官方界面变更之前落盘。若首次切换失败，会恢复原始模型并删除本次新增项；
若第三方模型之间切换失败，会恢复到本次操作前的第三方模型，而不是错误地退回首次接管
基线。任一步骤无法完整补偿时保留“待恢复”状态，不把半完成操作报告为成功。

Trae 的 Endpoint、模型 ID 和 API Key 在创建后不可编辑，因此输入变化时会原位替换同名
受管项；展示名仅使用 `Provider · 模型 ID`，所有权由当前检查点或同账号的认证加密历史中
记录的精确名称判定，不向 Trae 模型菜单暴露实现前缀。相同 Provider、模型、协议、地址和
Key 再次切换会复用已有项，不产生重复模型；旧版本创建的 `AT-Switch ·` 项会在下次成功
切换后按检查点所有权安全清理。若旧版本在界面已创建项目但未来得及完成检查点，则仅识别
符合旧版受管命名格式且与当前 Provider/模型输入精确匹配的项目，并在界面变更前先补入
加密检查点。历史恢复只接受使用本机凭据密钥认证、账号范围一致且内容哈希有效的记录，
不会把同名用户模型误判为 AT-Switch 所有。

若 Trae 已存在相同模型 ID，AT-Switch 直接选择该现有项，不再打开新增模型表单。该项以
“复用配置”记录在账号检查点中，用于校验当前选择，但不取得所有权；恢复原模型或切换到
其他模型时不会编辑、替换或删除它。

若 TraeCode 停留在模型管理页，或 TraeWork 停留在添加模型页，AT-Switch 会先自动返回
包含模型选择器的主界面，再继续复用或切换，无需用户手动关闭页面。
主界面当前官方模型未出现在 Trae 缓存时，适配器会读取主界面唯一的非空模型组合框；
添加模型页因带有明确标记，不会进入该兜底，避免把协议选择框误认为模型选择器。
TraeCode 的组合框使用控件自身位置触发菜单，不依赖窗口位置或固定屏幕坐标。

恢复时先选择首次接管前的模型，再删除检查点明确记录的 AT-Switch 模型项。用户原有
自定义模型、登录态、会话和未知设置均不修改。数据库提交失败时立即选回原模型；删除
失败时保留恢复检查点并标记待清理，下一次恢复继续执行，不能把半完成状态报告为健康。

## 自动化测试与实机验收

纯测试覆盖：稳定 ID、Direct/Proxy 协议限制、账号状态缺失、受管名称边界、Secret 不
进入检查点、首次切换、相同配置幂等、绑定提交失败补偿、恢复原选择，以及只删除
AT-Switch 所有项而保留用户模型；失败补偿同时覆盖首次接管和两个第三方模型之间切换。
前端测试覆盖三个官方协议、禁止 Proxy、首次确认、两个 Agent 独立显示，以及 Trae
页面不出现 ima 专属引导。

2026-10-08 macOS 实机使用两个一次性本地 Mock 模型，对 TraeCode 与 TraeWork 均完成：

- 打开官方模型设置，新增并选择第一个指向 `127.0.0.1` 的受管模型；
- 由 Trae 自己完成 OpenAI 兼容 SSE 连通性测试，并由 AT-Switch 回读主模型选择器；
- 从第一个第三方模型切换到第二个第三方模型，不产生重复受管项；
- 完全退出并重新启动 Trae 后，选择仍然保留且能被 AT-Switch 正确识别；
- 恢复到接管前的 `Auto` / `Auto Mode`，并精确删除两个 AT-Switch 受管项；
- 模拟中断后复用已经选中的受管项，恢复流程不创建重复项；
- 确认编辑页中的 Endpoint、模型 ID 和 API Key 为不可编辑字段，验证新增/复用设计；
- 确认官方删除按钮和确认流程可由语义化辅助功能控件定位。

TraeCode 另已实机确认设置入口、模型选择器和三种协议表单可由相同的语义化流程访问。
本轮未使用真实付费 Provider，也没有在默认新会话中验证真实上游回复和 Tool 调用；本地
Mock 的 SSE 只证明 Trae 连通性检查走通真实直连传输。Windows 已实现同语义的 UI
Automation、发现和恢复逻辑并通过自动化检查，但没有 Windows 真机结果，不能表述为
Windows 实机通过。
