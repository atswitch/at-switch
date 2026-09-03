use std::{
    collections::HashMap,
    convert::Infallible,
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc,
    },
};

use axum::{
    body::{Body, Bytes},
    extract::State,
    http::{header, HeaderMap, HeaderValue, StatusCode},
    response::{IntoResponse, Response},
    routing::{get, post},
    Router,
};
use chrono::Utc;
use futures_util::StreamExt;
use reqwest::Client;
use serde::Serialize;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use tokio::{
    net::TcpListener,
    sync::{oneshot, Mutex, RwLock},
    task::JoinHandle,
};

use crate::{
    domain::{ApiProtocol, AppResult, CommandError, ProxyRuntimeStatus, ProxyStatus},
    infrastructure::SecretStore,
    services::endpoint_url,
};

use super::{
    decode_request, decode_response, decode_stream_event, encode_request, encode_response,
    StreamEncoder,
};

/// Immutable routing data used by in-flight requests.
///
/// Credentials are deliberately represented by vault references. Raw upstream
/// API keys are fetched only for the lifetime of one request.
#[derive(Debug, Clone)]
pub struct RouteSnapshot {
    pub agent_id: String,
    pub source_protocol: ApiProtocol,
    pub upstream_protocol: ApiProtocol,
    pub upstream_base_url: String,
    pub upstream_model: String,
    pub upstream_api_key_ref: String,
}

#[derive(Default)]
pub struct RouteStore {
    by_token_hash: RwLock<HashMap<String, Arc<RouteSnapshot>>>,
}

impl RouteStore {
    #[allow(dead_code)]
    pub async fn replace(&self, routes: Vec<(String, RouteSnapshot)>) {
        let mut next = HashMap::with_capacity(routes.len());
        for (local_token, route) in routes {
            next.insert(hash_local_token(&local_token), Arc::new(route));
        }
        *self.by_token_hash.write().await = next;
    }

    #[allow(dead_code)]
    pub async fn register(&self, local_token: &str, route: RouteSnapshot) {
        self.by_token_hash
            .write()
            .await
            .insert(hash_local_token(local_token), Arc::new(route));
    }

    pub async fn unregister(&self, local_token: &str) {
        self.by_token_hash
            .write()
            .await
            .remove(&hash_local_token(local_token));
    }

    async fn resolve(&self, local_token: &str) -> Option<Arc<RouteSnapshot>> {
        self.by_token_hash
            .read()
            .await
            .get(&hash_local_token(local_token))
            .cloned()
    }
}

#[derive(Default)]
struct ProxyMetrics {
    active_connections: AtomicU64,
    completed_requests: AtomicU64,
    successful_requests: AtomicU64,
    conversion_failures: AtomicU64,
    upstream_failures: AtomicU64,
}

struct ProxyRuntime {
    status: ProxyRuntimeStatus,
    port: u16,
    started_at: Option<String>,
    error: Option<String>,
    shutdown: Option<oneshot::Sender<()>>,
    task: Option<JoinHandle<()>>,
}

impl ProxyRuntime {
    fn stopped(port: u16) -> Self {
        Self {
            status: ProxyRuntimeStatus::Stopped,
            port,
            started_at: None,
            error: None,
            shutdown: None,
            task: None,
        }
    }
}

pub struct ProxySupervisor {
    runtime: Mutex<ProxyRuntime>,
    routes: Arc<RouteStore>,
    secret_store: Arc<dyn SecretStore>,
    http: Client,
    metrics: Arc<ProxyMetrics>,
}

impl ProxySupervisor {
    pub fn new(port: u16, secret_store: Arc<dyn SecretStore>) -> AppResult<Arc<Self>> {
        let http = Client::builder()
            .connect_timeout(std::time::Duration::from_secs(15))
            .user_agent(format!("AT-Switch/{}", env!("CARGO_PKG_VERSION")))
            .build()
            .map_err(|error| {
                log::error!("proxy HTTP client initialization failed: {error}");
                CommandError::internal("无法初始化本地代理网络客户端")
            })?;
        Ok(Arc::new(Self {
            runtime: Mutex::new(ProxyRuntime::stopped(port)),
            routes: Arc::new(RouteStore::default()),
            secret_store,
            http,
            metrics: Arc::new(ProxyMetrics::default()),
        }))
    }

    #[allow(dead_code)]
    pub fn routes(&self) -> Arc<RouteStore> {
        Arc::clone(&self.routes)
    }

    pub async fn start(self: &Arc<Self>, port: u16) -> AppResult<ProxyStatus> {
        {
            let mut runtime = self.runtime.lock().await;
            if runtime.status == ProxyRuntimeStatus::Running && runtime.port == port {
                return Ok(self.status_from(&runtime));
            }
            if runtime.status != ProxyRuntimeStatus::Stopped
                && runtime.status != ProxyRuntimeStatus::Error
            {
                return Err(CommandError::new(
                    "proxy_transition_busy",
                    "本地代理正在切换状态，请稍后重试",
                ));
            }
            runtime.status = ProxyRuntimeStatus::Starting;
            runtime.port = port;
            runtime.error = None;
        }

        let address = format!("127.0.0.1:{port}");
        let listener = match TcpListener::bind(&address).await {
            Ok(listener) => listener,
            Err(error) => {
                log::warn!("proxy port bind failed for {address}: {error}");
                let mut runtime = self.runtime.lock().await;
                runtime.status = ProxyRuntimeStatus::Error;
                runtime.error = Some(format!("端口 {port} 无法使用"));
                return Err(CommandError::new(
                    "proxy_port_unavailable",
                    format!("无法监听本地端口 {port}"),
                )
                .with_recovery("请在代理设置中选择其他端口后重试。"));
            }
        };

        let state = ProxyServerState {
            routes: Arc::clone(&self.routes),
            secret_store: Arc::clone(&self.secret_store),
            http: self.http.clone(),
            metrics: Arc::clone(&self.metrics),
        };
        let router = build_router(state);
        let (shutdown_tx, shutdown_rx) = oneshot::channel();
        let supervisor = Arc::clone(self);
        let task = tokio::spawn(async move {
            let result = axum::serve(listener, router)
                .with_graceful_shutdown(async {
                    let _ = shutdown_rx.await;
                })
                .await;
            let mut runtime = supervisor.runtime.lock().await;
            if let Err(error) = result {
                log::error!("local proxy stopped unexpectedly: {error}");
                if runtime.status != ProxyRuntimeStatus::Draining {
                    runtime.status = ProxyRuntimeStatus::Error;
                    runtime.error = Some("本地代理异常停止".to_owned());
                }
            } else if runtime.status == ProxyRuntimeStatus::Running {
                runtime.status = ProxyRuntimeStatus::Error;
                runtime.error = Some("本地代理异常停止".to_owned());
            }
        });

        let mut runtime = self.runtime.lock().await;
        runtime.status = ProxyRuntimeStatus::Running;
        runtime.started_at = Some(Utc::now().to_rfc3339());
        runtime.shutdown = Some(shutdown_tx);
        runtime.task = Some(task);
        Ok(self.status_from(&runtime))
    }

    pub async fn stop(&self) -> AppResult<ProxyStatus> {
        let task = {
            let mut runtime = self.runtime.lock().await;
            if runtime.status == ProxyRuntimeStatus::Stopped {
                return Ok(self.status_from(&runtime));
            }
            runtime.status = ProxyRuntimeStatus::Draining;
            if let Some(shutdown) = runtime.shutdown.take() {
                let _ = shutdown.send(());
            }
            runtime.task.take()
        };
        if let Some(task) = task {
            if let Err(error) = task.await {
                log::warn!("proxy runtime join failed: {error}");
            }
        }
        let mut runtime = self.runtime.lock().await;
        runtime.status = ProxyRuntimeStatus::Stopped;
        runtime.started_at = None;
        runtime.error = None;
        Ok(self.status_from(&runtime))
    }

    pub async fn set_stopped_port(&self, port: u16) -> AppResult<ProxyStatus> {
        let mut runtime = self.runtime.lock().await;
        if !matches!(
            runtime.status,
            ProxyRuntimeStatus::Stopped | ProxyRuntimeStatus::Error
        ) {
            return Err(CommandError::new(
                "proxy_must_be_stopped",
                "修改端口前请先停止本地代理",
            ));
        }
        runtime.status = ProxyRuntimeStatus::Stopped;
        runtime.port = port;
        runtime.error = None;
        Ok(self.status_from(&runtime))
    }

    pub async fn status(&self) -> ProxyStatus {
        let runtime = self.runtime.lock().await;
        self.status_from(&runtime)
    }

    fn status_from(&self, runtime: &ProxyRuntime) -> ProxyStatus {
        ProxyStatus {
            status: runtime.status,
            host: "127.0.0.1".to_owned(),
            port: runtime.port,
            started_at: runtime.started_at.clone(),
            active_connections: self.metrics.active_connections.load(Ordering::Relaxed),
            completed_requests: self.metrics.completed_requests.load(Ordering::Relaxed),
            successful_requests: self.metrics.successful_requests.load(Ordering::Relaxed),
            conversion_failures: self.metrics.conversion_failures.load(Ordering::Relaxed),
            upstream_failures: self.metrics.upstream_failures.load(Ordering::Relaxed),
            error: runtime.error.clone(),
        }
    }
}

fn build_router(state: ProxyServerState) -> Router {
    Router::new()
        .route("/__at_switch/health", get(health))
        .route("/v1/chat/completions", post(chat_completions))
        .route("/v1/responses", post(responses))
        .route("/v1/messages", post(anthropic_messages))
        .with_state(state)
}

impl Drop for ProxySupervisor {
    fn drop(&mut self) {
        if let Ok(mut runtime) = self.runtime.try_lock() {
            if let Some(shutdown) = runtime.shutdown.take() {
                let _ = shutdown.send(());
            }
        }
    }
}

#[derive(Clone)]
struct ProxyServerState {
    routes: Arc<RouteStore>,
    secret_store: Arc<dyn SecretStore>,
    http: Client,
    metrics: Arc<ProxyMetrics>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct HealthResponse {
    status: &'static str,
    service: &'static str,
    version: &'static str,
}

async fn health() -> impl IntoResponse {
    axum::Json(HealthResponse {
        status: "ok",
        service: "at-switch-proxy",
        version: env!("CARGO_PKG_VERSION"),
    })
}

async fn chat_completions(
    State(state): State<ProxyServerState>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    handle_proxy_request(state, ApiProtocol::OpenaiChatCompletions, headers, body).await
}

async fn responses(
    State(state): State<ProxyServerState>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    handle_proxy_request(state, ApiProtocol::OpenaiResponses, headers, body).await
}

async fn anthropic_messages(
    State(state): State<ProxyServerState>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    handle_proxy_request(state, ApiProtocol::AnthropicMessages, headers, body).await
}

async fn handle_proxy_request(
    state: ProxyServerState,
    source_protocol: ApiProtocol,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    let connection = ActiveConnection::new(Arc::clone(&state.metrics));
    let local_token = match extract_local_token(&headers) {
        Some(token) => token,
        None => {
            return proxy_error(
                StatusCode::UNAUTHORIZED,
                "local_token_missing",
                "缺少本地代理令牌",
            )
        }
    };
    let route = match state.routes.resolve(local_token).await {
        Some(route) if route.source_protocol == source_protocol => route,
        Some(_) => {
            return proxy_error(
                StatusCode::BAD_REQUEST,
                "route_protocol_mismatch",
                "请求路径与 Agent 路由协议不匹配",
            )
        }
        None => {
            return proxy_error(
                StatusCode::UNAUTHORIZED,
                "local_token_invalid",
                "本地代理令牌无效",
            )
        }
    };

    let mut source_value: Value = match serde_json::from_slice(&body) {
        Ok(value) => value,
        Err(_) => {
            state
                .metrics
                .conversion_failures
                .fetch_add(1, Ordering::Relaxed);
            return proxy_error(
                StatusCode::BAD_REQUEST,
                "invalid_json",
                "请求体不是有效的 JSON",
            );
        }
    };
    let wants_stream = source_value
        .get("stream")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let same_protocol = source_protocol == route.upstream_protocol;
    let upstream_body = if same_protocol {
        // A same-protocol route must remain transparent. In particular, Codex
        // sends Responses fields and native tool declarations that the
        // cross-protocol canonical model intentionally does not interpret.
        source_value["model"] = Value::String(route.upstream_model.clone());
        source_value
    } else {
        let mut canonical = match decode_request(source_protocol, &source_value) {
            Ok(request) => request,
            Err(error) => {
                state
                    .metrics
                    .conversion_failures
                    .fetch_add(1, Ordering::Relaxed);
                return command_error_response(StatusCode::UNPROCESSABLE_ENTITY, error);
            }
        };
        canonical.model.clone_from(&route.upstream_model);
        match encode_request(route.upstream_protocol, &canonical) {
            Ok(body) => body,
            Err(error) => {
                state
                    .metrics
                    .conversion_failures
                    .fetch_add(1, Ordering::Relaxed);
                return command_error_response(StatusCode::UNPROCESSABLE_ENTITY, error);
            }
        }
    };
    let upstream_key = match state.secret_store.get(&route.upstream_api_key_ref) {
        Ok(secret) => secret,
        Err(error) => return command_error_response(StatusCode::BAD_GATEWAY, error),
    };
    let endpoint = match protocol_endpoint(route.upstream_protocol)
        .and_then(|path| endpoint_url(&route.upstream_base_url, path))
    {
        Ok(endpoint) => endpoint,
        Err(error) => return command_error_response(StatusCode::BAD_GATEWAY, error),
    };
    let mut request = state
        .http
        .post(endpoint)
        .header(header::CONTENT_TYPE, "application/json")
        .json(&upstream_body);
    request = match route.upstream_protocol {
        ApiProtocol::AnthropicMessages => request
            .header("x-api-key", upstream_key.expose())
            .header("anthropic-version", "2023-06-01"),
        _ => request.bearer_auth(upstream_key.expose()),
    };

    let upstream = match request.send().await {
        Ok(response) => response,
        Err(error) => {
            log::warn!(
                "proxy upstream transport failed for agent {}: {}",
                route.agent_id,
                error
            );
            state
                .metrics
                .upstream_failures
                .fetch_add(1, Ordering::Relaxed);
            return proxy_error(
                StatusCode::BAD_GATEWAY,
                "upstream_unreachable",
                "无法连接到上游 Provider",
            );
        }
    };
    if !upstream.status().is_success() {
        let status =
            StatusCode::from_u16(upstream.status().as_u16()).unwrap_or(StatusCode::BAD_GATEWAY);
        state
            .metrics
            .upstream_failures
            .fetch_add(1, Ordering::Relaxed);
        return proxy_error(status, "upstream_rejected", "上游 Provider 拒绝了请求");
    }

    if same_protocol && wants_stream {
        passthrough_stream_response(upstream, Arc::clone(&state.metrics), connection)
    } else if same_protocol {
        passthrough_non_stream_response(upstream, &state.metrics).await
    } else if wants_stream {
        stream_response(
            upstream,
            source_protocol,
            route.upstream_protocol,
            Arc::clone(&state.metrics),
            connection,
        )
    } else {
        non_stream_response(
            upstream,
            source_protocol,
            route.upstream_protocol,
            &state.metrics,
        )
        .await
    }
}

async fn passthrough_non_stream_response(
    upstream: reqwest::Response,
    metrics: &ProxyMetrics,
) -> Response {
    let status =
        StatusCode::from_u16(upstream.status().as_u16()).unwrap_or(StatusCode::BAD_GATEWAY);
    let content_type = upstream.headers().get(header::CONTENT_TYPE).cloned();
    let body = match upstream.bytes().await {
        Ok(body) => body,
        Err(error) => {
            log::warn!("proxy upstream body passthrough failed: {error}");
            metrics.upstream_failures.fetch_add(1, Ordering::Relaxed);
            return proxy_error(
                StatusCode::BAD_GATEWAY,
                "upstream_response_invalid",
                "读取上游响应失败",
            );
        }
    };
    metrics.completed_requests.fetch_add(1, Ordering::Relaxed);
    metrics.successful_requests.fetch_add(1, Ordering::Relaxed);
    let mut response = Response::new(Body::from(body));
    *response.status_mut() = status;
    if let Some(content_type) = content_type {
        response
            .headers_mut()
            .insert(header::CONTENT_TYPE, content_type);
    }
    response
}

fn passthrough_stream_response(
    upstream: reqwest::Response,
    metrics: Arc<ProxyMetrics>,
    connection: ActiveConnection,
) -> Response {
    let content_type = upstream.headers().get(header::CONTENT_TYPE).cloned();
    let stream = async_stream::stream! {
        let _connection = connection;
        let mut upstream_stream = upstream.bytes_stream();
        let mut failed = false;
        while let Some(chunk) = upstream_stream.next().await {
            match chunk {
                Ok(chunk) => yield Ok::<Bytes, Infallible>(chunk),
                Err(error) => {
                    log::warn!("proxy upstream stream passthrough failed: {error}");
                    metrics.upstream_failures.fetch_add(1, Ordering::Relaxed);
                    failed = true;
                    break;
                }
            }
        }
        metrics.completed_requests.fetch_add(1, Ordering::Relaxed);
        if !failed {
            metrics.successful_requests.fetch_add(1, Ordering::Relaxed);
        }
    };
    let mut response = Response::new(Body::from_stream(stream));
    *response.status_mut() = StatusCode::OK;
    response.headers_mut().insert(
        header::CONTENT_TYPE,
        content_type
            .unwrap_or_else(|| HeaderValue::from_static("text/event-stream; charset=utf-8")),
    );
    response
        .headers_mut()
        .insert(header::CACHE_CONTROL, HeaderValue::from_static("no-cache"));
    response
}

async fn non_stream_response(
    upstream: reqwest::Response,
    source_protocol: ApiProtocol,
    upstream_protocol: ApiProtocol,
    metrics: &ProxyMetrics,
) -> Response {
    let upstream_value: Value = match upstream.json().await {
        Ok(value) => value,
        Err(error) => {
            log::warn!("proxy upstream JSON could not be decoded: {error}");
            metrics.upstream_failures.fetch_add(1, Ordering::Relaxed);
            return proxy_error(
                StatusCode::BAD_GATEWAY,
                "upstream_response_invalid",
                "上游返回了无法解析的响应",
            );
        }
    };
    let canonical = match decode_response(upstream_protocol, &upstream_value) {
        Ok(response) => response,
        Err(error) => {
            metrics.conversion_failures.fetch_add(1, Ordering::Relaxed);
            return command_error_response(StatusCode::BAD_GATEWAY, error);
        }
    };
    let source_value = match encode_response(source_protocol, &canonical) {
        Ok(value) => value,
        Err(error) => {
            metrics.conversion_failures.fetch_add(1, Ordering::Relaxed);
            return command_error_response(StatusCode::BAD_GATEWAY, error);
        }
    };
    metrics.completed_requests.fetch_add(1, Ordering::Relaxed);
    metrics.successful_requests.fetch_add(1, Ordering::Relaxed);
    (StatusCode::OK, axum::Json(source_value)).into_response()
}

fn stream_response(
    upstream: reqwest::Response,
    source_protocol: ApiProtocol,
    upstream_protocol: ApiProtocol,
    metrics: Arc<ProxyMetrics>,
    connection: ActiveConnection,
) -> Response {
    let stream = async_stream::stream! {
        let _connection = connection;
        let mut upstream_stream = upstream.bytes_stream();
        let mut buffer = Vec::<u8>::new();
        let mut failed = false;
        let mut encoder = StreamEncoder::new(source_protocol);
        while let Some(chunk) = upstream_stream.next().await {
            match chunk {
                Ok(chunk) => buffer.extend_from_slice(&chunk),
                Err(error) => {
                    log::warn!("proxy upstream stream failed: {error}");
                    metrics.upstream_failures.fetch_add(1, Ordering::Relaxed);
                    failed = true;
                    break;
                }
            }
            while let Some((boundary, delimiter_length)) = find_sse_boundary(&buffer) {
                let frame = buffer.drain(..boundary).collect::<Vec<_>>();
                buffer.drain(..delimiter_length);
                match transcode_sse_frame(&frame, upstream_protocol, &mut encoder) {
                    Ok(encoded_frames) => {
                        for encoded in encoded_frames {
                            yield Ok::<Bytes, Infallible>(Bytes::from(encoded));
                        }
                    }
                    Err(error) => {
                        log::warn!("proxy stream conversion failed: {error}");
                        metrics.conversion_failures.fetch_add(1, Ordering::Relaxed);
                        failed = true;
                        break;
                    }
                }
            }
            if failed {
                break;
            }
        }
        if !failed && !buffer.is_empty() {
            match transcode_sse_frame(&buffer, upstream_protocol, &mut encoder) {
                Ok(encoded_frames) => {
                    for encoded in encoded_frames {
                        yield Ok::<Bytes, Infallible>(Bytes::from(encoded));
                    }
                }
                Err(error) => {
                    log::warn!("proxy final stream event conversion failed: {error}");
                    metrics.conversion_failures.fetch_add(1, Ordering::Relaxed);
                    failed = true;
                }
            }
        }
        metrics.completed_requests.fetch_add(1, Ordering::Relaxed);
        if !failed {
            metrics.successful_requests.fetch_add(1, Ordering::Relaxed);
        }
    };

    let mut response = Response::new(Body::from_stream(stream));
    *response.status_mut() = StatusCode::OK;
    response.headers_mut().insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("text/event-stream; charset=utf-8"),
    );
    response
        .headers_mut()
        .insert(header::CACHE_CONTROL, HeaderValue::from_static("no-cache"));
    response
}

fn transcode_sse_frame(
    frame: &[u8],
    upstream_protocol: ApiProtocol,
    encoder: &mut StreamEncoder,
) -> AppResult<Vec<String>> {
    let text = std::str::from_utf8(frame)
        .map_err(|_| CommandError::new("invalid_stream_encoding", "上游流事件不是有效的 UTF-8"))?;
    let mut event_name = None;
    let mut data_lines = Vec::new();
    for line in text.lines() {
        if let Some(value) = line.strip_prefix("event:") {
            event_name = Some(value.trim());
        } else if let Some(value) = line.strip_prefix("data:") {
            data_lines.push(value.trim_start());
        }
    }
    if data_lines.is_empty() {
        return Ok(Vec::new());
    }
    let data = data_lines.join("\n");
    let canonical_events = decode_stream_event(upstream_protocol, event_name, &data)?;
    let mut encoded = Vec::new();
    for event in &canonical_events {
        encoded.extend(encoder.encode(event)?);
    }
    Ok(encoded)
}

fn find_sse_boundary(buffer: &[u8]) -> Option<(usize, usize)> {
    if let Some(index) = buffer.windows(4).position(|window| window == b"\r\n\r\n") {
        return Some((index, 4));
    }
    buffer
        .windows(2)
        .position(|window| window == b"\n\n")
        .map(|index| (index, 2))
}

fn extract_local_token(headers: &HeaderMap) -> Option<&str> {
    headers
        .get(header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.strip_prefix("Bearer "))
        .or_else(|| {
            headers
                .get("x-api-key")
                .and_then(|value| value.to_str().ok())
        })
        .filter(|value| !value.is_empty())
}

fn protocol_endpoint(protocol: ApiProtocol) -> AppResult<&'static str> {
    Ok(match protocol {
        ApiProtocol::OpenaiChatCompletions => "chat/completions",
        ApiProtocol::OpenaiResponses => "responses",
        ApiProtocol::AnthropicMessages => "messages",
    })
}

fn hash_local_token(token: &str) -> String {
    hex::encode(Sha256::digest(token.as_bytes()))
}

struct ActiveConnection {
    metrics: Arc<ProxyMetrics>,
}

impl ActiveConnection {
    fn new(metrics: Arc<ProxyMetrics>) -> Self {
        metrics.active_connections.fetch_add(1, Ordering::Relaxed);
        Self { metrics }
    }
}

impl Drop for ActiveConnection {
    fn drop(&mut self) {
        self.metrics
            .active_connections
            .fetch_sub(1, Ordering::Relaxed);
    }
}

fn command_error_response(status: StatusCode, error: CommandError) -> Response {
    proxy_error(status, &error.code, &error.message)
}

fn proxy_error(status: StatusCode, code: &str, message: &str) -> Response {
    (
        status,
        axum::Json(json!({
            "error": {
                "type": code,
                "message": message
            }
        })),
    )
        .into_response()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::infrastructure::{MemorySecretStore, SecretValue};
    use crate::proxy::{CanonicalStreamEvent, CanonicalUsage};
    use axum::http::Request;
    use std::{fs, path::PathBuf, process::Command};
    use tempfile::tempdir;
    use tower::ServiceExt;

    fn test_router() -> Router {
        build_router(ProxyServerState {
            routes: Arc::new(RouteStore::default()),
            secret_store: Arc::new(MemorySecretStore::default()),
            http: Client::new(),
            metrics: Arc::new(ProxyMetrics::default()),
        })
    }

    #[tokio::test]
    async fn route_store_never_indexes_raw_tokens() {
        let store = RouteStore::default();
        store
            .register(
                "atsw_local_secret",
                RouteSnapshot {
                    agent_id: "codex".to_owned(),
                    source_protocol: ApiProtocol::OpenaiResponses,
                    upstream_protocol: ApiProtocol::AnthropicMessages,
                    upstream_base_url: "https://provider.example/v1".to_owned(),
                    upstream_model: "model-a".to_owned(),
                    upstream_api_key_ref: "provider/p/api-key/v1".to_owned(),
                },
            )
            .await;

        assert!(store.resolve("atsw_local_secret").await.is_some());
        assert!(store.resolve("wrong").await.is_none());
        assert!(!store
            .by_token_hash
            .read()
            .await
            .contains_key("atsw_local_secret"));
    }

    #[test]
    fn parses_crlf_and_lf_sse_boundaries() {
        assert_eq!(find_sse_boundary(b"data: {}\n\nrest"), Some((8, 2)));
        assert_eq!(
            find_sse_boundary(b"event: ping\r\ndata: {}\r\n\r\nrest"),
            Some((21, 4))
        );
    }

    #[tokio::test]
    async fn health_endpoint_is_available_without_credentials() {
        let response = test_router()
            .oneshot(
                Request::builder()
                    .uri("/__at_switch/health")
                    .body(Body::empty())
                    .expect("request"),
            )
            .await
            .expect("response");
        assert_eq!(response.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn business_endpoint_rejects_missing_local_token() {
        let response = test_router()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/v1/responses")
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from(r#"{"model":"test","input":"hello"}"#))
                    .expect("request"),
            )
            .await
            .expect("response");
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn a_new_port_recovers_from_a_previous_bind_error() {
        let supervisor = ProxySupervisor::new(54187, Arc::new(MemorySecretStore::default()))
            .expect("supervisor");
        {
            let mut runtime = supervisor.runtime.lock().await;
            runtime.status = ProxyRuntimeStatus::Error;
            runtime.error = Some("端口不可用".to_owned());
        }

        let status = supervisor.set_stopped_port(54188).await.expect("port");
        assert_eq!(status.status, ProxyRuntimeStatus::Stopped);
        assert_eq!(status.port, 54188);
        assert!(status.error.is_none());
    }

    /// Runs the installed Codex CLI against a real AT-Switch HTTP listener and
    /// a deterministic native Responses provider.
    ///
    /// This is ignored in normal CI because it requires a Codex installation.
    /// Run it explicitly before releasing Codex protocol changes:
    ///
    /// `AT_SWITCH_CODEX_BIN=/path/to/codex cargo test
    /// proxy::server::tests::installed_codex_renders_native_responses_passthrough
    /// -- --ignored --nocapture`
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    #[ignore = "requires an installed Codex CLI"]
    async fn installed_codex_renders_native_responses_passthrough() {
        let mut upstream_encoder = StreamEncoder::new(ApiProtocol::OpenaiResponses);
        let mut upstream_frames = Vec::new();
        for event in [
            CanonicalStreamEvent::MessageStart {
                id: Some("resp_at_switch_e2e".to_owned()),
                model: Some("at-switch-e2e".to_owned()),
            },
            CanonicalStreamEvent::TextDelta {
                text: "AT_SWITCH_".to_owned(),
            },
            CanonicalStreamEvent::TextDelta {
                text: "E2E_OK".to_owned(),
            },
            CanonicalStreamEvent::Usage {
                usage: CanonicalUsage {
                    input_tokens: Some(3),
                    output_tokens: Some(4),
                    cache_read_tokens: Some(0),
                },
            },
            CanonicalStreamEvent::MessageEnd {
                finish_reason: Some("stop".to_owned()),
            },
        ] {
            upstream_frames.extend(upstream_encoder.encode(&event).expect("Responses fixture"));
        }
        let upstream_body = Arc::new(upstream_frames.concat());
        let upstream = Router::new().route(
            "/v1/responses",
            post(move || {
                let body = Arc::clone(&upstream_body);
                async move {
                    (
                        [(header::CONTENT_TYPE, "text/event-stream; charset=utf-8")],
                        body.as_str().to_owned(),
                    )
                }
            }),
        );
        let upstream_listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("upstream listener");
        let upstream_address = upstream_listener.local_addr().expect("upstream address");
        let upstream_task = tokio::spawn(async move {
            axum::serve(upstream_listener, upstream)
                .await
                .expect("upstream server");
        });

        let local_token = "at-switch-e2e-local-token";
        let secret_reference = "provider/at-switch-e2e/api-key/v1";
        let secret_store = MemorySecretStore::default();
        secret_store
            .put(
                secret_reference,
                &SecretValue::new("at-switch-e2e-upstream-key".to_owned()),
            )
            .expect("store upstream key");
        let routes = Arc::new(RouteStore::default());
        routes
            .register(
                local_token,
                RouteSnapshot {
                    agent_id: "codex".to_owned(),
                    source_protocol: ApiProtocol::OpenaiResponses,
                    upstream_protocol: ApiProtocol::OpenaiResponses,
                    upstream_base_url: format!("http://{upstream_address}/v1"),
                    upstream_model: "at-switch-e2e".to_owned(),
                    upstream_api_key_ref: secret_reference.to_owned(),
                },
            )
            .await;
        let proxy = build_router(ProxyServerState {
            routes,
            secret_store: Arc::new(secret_store),
            http: Client::new(),
            metrics: Arc::new(ProxyMetrics::default()),
        });
        let proxy_listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("proxy listener");
        let proxy_address = proxy_listener.local_addr().expect("proxy address");
        let proxy_task = tokio::spawn(async move {
            axum::serve(proxy_listener, proxy)
                .await
                .expect("proxy server");
        });

        let codex_bin = std::env::var_os("AT_SWITCH_CODEX_BIN")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("/Applications/ChatGPT.app/Contents/Resources/codex"));
        assert!(
            codex_bin.is_file(),
            "set AT_SWITCH_CODEX_BIN to the installed Codex CLI"
        );
        let home = tempdir().expect("temporary Codex home");
        let last_message = home.path().join("last-message.txt");
        let config = format!(
            r#"model = "at-switch-e2e"
model_provider = "at_switch_e2e"
approval_policy = "never"
sandbox_mode = "read-only"

[model_providers.at_switch_e2e]
name = "AT-Switch E2E"
base_url = "http://{proxy_address}/v1"
wire_api = "responses"
requires_openai_auth = false
experimental_bearer_token = "{local_token}"
"#
        );
        fs::write(home.path().join("config.toml"), config).expect("write Codex config");

        let output = Command::new(codex_bin)
            .env("CODEX_HOME", home.path())
            .args([
                "exec",
                "--json",
                "--ephemeral",
                "--skip-git-repo-check",
                "--ignore-rules",
                "--sandbox",
                "read-only",
                "--output-last-message",
            ])
            .arg(&last_message)
            .arg("Reply exactly with AT_SWITCH_E2E_OK and do not call tools.")
            .current_dir(home.path())
            .output()
            .expect("run installed Codex");

        proxy_task.abort();
        upstream_task.abort();

        assert!(
            output.status.success(),
            "Codex failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert_eq!(
            fs::read_to_string(last_message)
                .expect("read final Codex message")
                .trim(),
            "AT_SWITCH_E2E_OK"
        );
    }
}
