# Provider 图标资产

Provider 列表使用本目录中的 PNG 或 SVG 图标识别对应服务。运行时只打包
`ProviderLogo.tsx` 明确导入的文件，不从第三方网站动态下载图标。

当前包含 DeepSeek、Doubao、Kimi/Moonshot AI、MiniMax、Mongyun、Qwen 和 Zhipu AI
的识别素材。维护者已确认这些素材可以随项目分发；产品名、图标和商标仍归各自
权利人所有，不属于本仓库 MIT 许可证的授权范围。

更新素材时必须使用真实图片或 SVG 文件，检查 MIME 类型，不得把下载失败返回的
HTML 页面、空文件或第三方跟踪脚本提交为图片。完整说明见
[第三方声明](../../../THIRD_PARTY_NOTICES.md)。
