/// 服务端地址：只来自打包期注入的环境变量（`.env.build` → beforeBuildCommand），
/// 源码中不出现任何域名或 IP。未注入时账号功能整体不可用，本地功能不受影响。
pub fn api_base() -> Option<&'static str> {
    match option_env!("AT_SWITCH_API_BASE") {
        Some(value) if !value.trim().is_empty() => Some(value),
        _ => None,
    }
}
