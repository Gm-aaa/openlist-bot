//! Self-hosted, same-origin chat UI. All API calls require a custom header;
//! browsers cannot submit it cross-origin without a preflight (no CORS allowed).
use crate::{config::WebConfig, BotContext};
use argon2::{password_hash::SaltString, Argon2, PasswordHash, PasswordHasher, PasswordVerifier};
use axum::{
    body::Bytes,
    extract::{ConnectInfo, DefaultBodyLimit, Query, Request, State},
    http::{HeaderMap, HeaderValue, StatusCode},
    middleware::{self, Next},
    response::{Html, IntoResponse, Response},
    routing::{get, post},
    Json, Router,
};
use rand::{rngs::OsRng, RngCore};
use serde::Deserialize;
use serde_json::{json, Value};
use std::{
    collections::HashMap,
    net::{IpAddr, SocketAddr},
    sync::Arc,
    time::{Duration, Instant},
};
use tokio::sync::{Mutex, Semaphore};

type Error = (StatusCode, Json<Value>);
type Result<T> = std::result::Result<T, Error>;
const SESSION_TTL: Duration = Duration::from_secs(8 * 3600);
const UPLOAD_LIMIT: usize = 16 * 1024 * 1024;

fn err(status: StatusCode, message: &str) -> Error {
    (status, Json(json!({"error": message})))
}
fn bad(message: &str) -> Error {
    err(StatusCode::BAD_REQUEST, message)
}
fn upstream(_: impl std::fmt::Display) -> Error {
    err(
        StatusCode::BAD_GATEWAY,
        "上游服务操作失败，请检查 OpenList / PanSou 的连接、权限和任务状态。",
    )
}

pub fn validate_config(
    config: &WebConfig,
) -> std::result::Result<(), Box<dyn std::error::Error + Send + Sync>> {
    if config.username.trim().is_empty() {
        return Err("web.username cannot be empty".into());
    }
    if config.password.is_some() != config.password_hash.is_empty() {
        return Err("Configure exactly one of web.password or web.password_hash".into());
    }
    if let Some(password) = &config.password {
        if password.is_empty() || password.len() > 1024 {
            return Err("web.password must contain 1–1024 bytes".into());
        }
    }
    for hash in std::iter::once(&config.password_hash)
        .filter(|hash| !hash.is_empty())
        .chain(config.secondary_password_hash.iter())
    {
        let parsed =
            PasswordHash::new(hash).map_err(|_| "Invalid web password hash; use hash-password")?;
        if parsed.algorithm.as_str() != "argon2id" || parsed.hash.is_none() || parsed.salt.is_none()
        {
            return Err(
                "Web passwords must be Argon2id hashes generated with hash-password".into(),
            );
        }
    }
    config.bind.parse::<SocketAddr>()?;
    Ok(())
}

pub fn hash_password_command() -> std::result::Result<(), Box<dyn std::error::Error + Send + Sync>>
{
    let password = rpassword::prompt_password("密码（至少 12 字符）: ")?;
    if password.chars().count() < 12 || password.len() > 1024 {
        return Err("Password must be 12–1024 characters/bytes".into());
    }
    let confirm = rpassword::prompt_password("再次输入: ")?;
    if password != confirm {
        return Err("Passwords do not match".into());
    }
    let salt = SaltString::generate(&mut OsRng);
    let hash = Argon2::default()
        .hash_password(password.as_bytes(), &salt)
        .map_err(|e| e.to_string())?;
    println!("{}", hash);
    Ok(())
}

struct Session {
    expires: Instant,
}
struct Attempt {
    since: Instant,
    count: u32,
}
struct WebState {
    ctx: Arc<BotContext>,
    config: WebConfig,
    sessions: Mutex<HashMap<String, Session>>,
    attempts: Mutex<HashMap<IpAddr, Attempt>>,
    hashing: Arc<Semaphore>,
    operations: Semaphore,
    upload_lock: Mutex<()>,
}

fn routes(state: Arc<WebState>) -> Router {
    Router::new()
        .route("/", get(|| async { Html(include_str!("web/index.html")) }))
        .route(
            "/app.css",
            get(|| async {
                (
                    [("content-type", "text/css; charset=utf-8")],
                    include_str!("web/app.css"),
                )
            }),
        )
        .route(
            "/app.js",
            get(|| async {
                (
                    [("content-type", "text/javascript; charset=utf-8")],
                    include_str!("web/app.js"),
                )
            }),
        )
        .route("/api/login", post(login))
        .route("/api/session", post(session))
        .route("/api/logout", post(logout))
        .route("/api/action", post(action))
        .layer(DefaultBodyLimit::max(32 * 1024))
        .route(
            "/api/upload",
            post(upload).layer(DefaultBodyLimit::max(UPLOAD_LIMIT)),
        )
        .layer(middleware::from_fn_with_state(
            state.clone(),
            authenticate_request,
        ))
        .layer(middleware::from_fn(security_headers))
        .with_state(state)
}

// Authenticate before collecting JSON or upload bodies.
async fn authenticate_request(
    State(state): State<Arc<WebState>>,
    request: Request,
    next: Next,
) -> Response {
    let path = request.uri().path();
    if path.starts_with("/api/") && path != "/api/login" && path != "/api/logout" {
        if let Err(error) = state.authorize(request.headers()).await {
            return error.into_response();
        }
    }
    next.run(request).await
}

#[cfg(test)]
mod tests;

pub async fn start(
    ctx: Arc<BotContext>,
) -> std::result::Result<
    Option<tokio::task::JoinHandle<std::io::Result<()>>>,
    Box<dyn std::error::Error + Send + Sync>,
> {
    let config = match ctx.config.read().await.web.clone() {
        Some(c) => c,
        None => return Ok(None),
    };
    validate_config(&config)?;
    let listener = tokio::net::TcpListener::bind(&config.bind).await?;
    tracing::info!("Web chat listening on {}", listener.local_addr()?);
    let state = Arc::new(WebState {
        ctx,
        config,
        sessions: Mutex::new(HashMap::new()),
        attempts: Mutex::new(HashMap::new()),
        hashing: Arc::new(Semaphore::new(2)),
        operations: Semaphore::new(16),
        upload_lock: Mutex::new(()),
    });
    Ok(Some(tokio::spawn(async move {
        axum::serve(
            listener,
            routes(state).into_make_service_with_connect_info::<SocketAddr>(),
        )
        .await
    })))
}

async fn security_headers(request: Request, next: Next) -> Response {
    let mut response = next.run(request).await;
    for (key, value) in [
        ("cache-control", "no-store"),
        ("x-content-type-options", "nosniff"),
        ("x-frame-options", "DENY"),
        ("referrer-policy", "no-referrer"),
        ("content-security-policy", "default-src 'none'; script-src 'self'; style-src 'self'; connect-src 'self'; img-src 'self' data:; font-src 'self'; base-uri 'none'; form-action 'self'; frame-ancestors 'none'"),
    ] { response.headers_mut().insert(key, HeaderValue::from_static(value)); }
    response
}

fn check_header(headers: &HeaderMap) -> Result<()> {
    if headers.get("x-openlist-web").and_then(|v| v.to_str().ok()) != Some("1") {
        return Err(err(StatusCode::FORBIDDEN, "请求来源验证失败"));
    }
    if headers.get("sec-fetch-site").and_then(|v| v.to_str().ok()) == Some("cross-site") {
        return Err(err(StatusCode::FORBIDDEN, "不允许跨站请求"));
    }
    Ok(())
}

fn token(headers: &HeaderMap) -> Option<&str> {
    headers
        .get("cookie")?
        .to_str()
        .ok()?
        .split(';')
        .find_map(|part| part.trim().strip_prefix("openlist_session="))
}

fn cookie(config: &WebConfig, value: &str, age: u64) -> HeaderValue {
    HeaderValue::from_str(&format!(
        "openlist_session={}; HttpOnly; SameSite=Strict; Path=/; Max-Age={}{}",
        value,
        age,
        if config.cookie_secure { "; Secure" } else { "" }
    ))
    .unwrap()
}

impl WebState {
    async fn authorize(&self, headers: &HeaderMap) -> Result<()> {
        check_header(headers)?;
        let mut sessions = self.sessions.lock().await;
        sessions.retain(|_, s| s.expires > Instant::now());
        if !token(headers)
            .map(|t| sessions.contains_key(t))
            .unwrap_or(false)
        {
            return Err(err(
                StatusCode::UNAUTHORIZED,
                "请先登录，或重新登录已过期的会话",
            ));
        }
        Ok(())
    }

    async fn throttle(&self, ip: IpAddr) -> Result<()> {
        let mut attempts = self.attempts.lock().await;
        attempts.retain(|_, a| a.since.elapsed() < Duration::from_secs(60));
        if attempts.len() >= 1024 && !attempts.contains_key(&ip) {
            return Err(err(
                StatusCode::TOO_MANY_REQUESTS,
                "请求过多，请一分钟后重试",
            ));
        }
        let entry = attempts.entry(ip).or_insert(Attempt {
            since: Instant::now(),
            count: 0,
        });
        entry.count += 1;
        if entry.count > 10 {
            return Err(err(
                StatusCode::TOO_MANY_REQUESTS,
                "验证过于频繁，请一分钟后重试",
            ));
        }
        Ok(())
    }

    async fn verify(&self, hash: String, password: String) -> Result<bool> {
        if password.len() > 1024 {
            return Err(bad("密码过长"));
        }
        let permit = self.hashing.clone().try_acquire_owned().map_err(|_| {
            err(
                StatusCode::TOO_MANY_REQUESTS,
                "正在验证其他请求，请稍后重试",
            )
        })?;
        tokio::task::spawn_blocking(move || {
            let _permit = permit;
            PasswordHash::new(&hash)
                .map(|h| {
                    Argon2::default()
                        .verify_password(password.as_bytes(), &h)
                        .is_ok()
                })
                .unwrap_or(false)
        })
        .await
        .map_err(|_| err(StatusCode::INTERNAL_SERVER_ERROR, "密码验证失败"))
    }

    async fn secondary(&self, ip: IpAddr, password: Option<String>) -> Result<()> {
        if let Some(hash) = &self.config.secondary_password_hash {
            let password =
                password.ok_or_else(|| err(StatusCode::FORBIDDEN, "此操作需要二级密码"))?;
            self.throttle(ip).await?;
            if !self.verify(hash.clone(), password).await? {
                return Err(err(StatusCode::FORBIDDEN, "二级密码错误"));
            }
        }
        Ok(())
    }
}

#[derive(Deserialize)]
struct Credentials {
    username: String,
    password: String,
}

async fn login(
    State(state): State<Arc<WebState>>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Json(input): Json<Credentials>,
) -> Result<Response> {
    check_header(&headers)?;
    state.throttle(peer.ip()).await?;
    let correct = if let Some(password) = &state.config.password {
        use subtle::ConstantTimeEq;
        if input.password.len() > 1024 {
            return Err(bad("密码过长"));
        }
        bool::from(password.as_bytes().ct_eq(input.password.as_bytes()))
    } else {
        state
            .verify(state.config.password_hash.clone(), input.password)
            .await?
    };
    if !correct || input.username != state.config.username {
        return Err(err(StatusCode::UNAUTHORIZED, "用户名或密码错误"));
    }
    let mut bytes = [0u8; 32];
    OsRng.fill_bytes(&mut bytes);
    let id: String = bytes.iter().map(|b| format!("{:02x}", b)).collect();
    let mut sessions = state.sessions.lock().await;
    sessions.retain(|_, s| s.expires > Instant::now());
    if let Some(old) = token(&headers) {
        sessions.remove(old);
    }
    if sessions.len() >= 256 {
        return Err(err(
            StatusCode::TOO_MANY_REQUESTS,
            "登录会话过多，请稍后重试",
        ));
    }
    sessions.insert(
        id.clone(),
        Session {
            expires: Instant::now() + SESSION_TTL,
        },
    );
    let mut response = Json(json!({"ok": true})).into_response();
    response.headers_mut().insert(
        "set-cookie",
        cookie(&state.config, &id, SESSION_TTL.as_secs()),
    );
    Ok(response)
}

async fn session(State(state): State<Arc<WebState>>, headers: HeaderMap) -> Result<Json<Value>> {
    state.authorize(&headers).await?;
    let config = state.ctx.config.read().await;
    Ok(Json(
        json!({"username": state.config.username, "secondary_required": state.config.secondary_password_hash.is_some(), "download_path": config.openlist.download_path, "download_tool": config.openlist.download_tool, "allowed_sources": config.search.allowed_sources, "pansou_enabled": config.pansou.is_some(), "upload_limit": UPLOAD_LIMIT}),
    ))
}

async fn logout(State(state): State<Arc<WebState>>, headers: HeaderMap) -> Result<Response> {
    check_header(&headers)?;
    if let Some(id) = token(&headers) {
        state.sessions.lock().await.remove(id);
    }
    let mut response = Json(json!({"ok": true})).into_response();
    response
        .headers_mut()
        .insert("set-cookie", cookie(&state.config, "", 0));
    Ok(response)
}

#[derive(Deserialize, Default)]
struct Action {
    action: String,
    #[serde(default)]
    path: String,
    #[serde(default)]
    name: String,
    #[serde(default)]
    keyword: String,
    #[serde(default)]
    tool: String,
    #[serde(default)]
    urls: Vec<String>,
    #[serde(default)]
    allowed_sources: Vec<String>,
    secondary_password: Option<String>,
}

fn valid_path(path: &str) -> Result<()> {
    if path.len() > 4096
        || !path.starts_with('/')
        || path.contains('\\')
        || path.chars().any(char::is_control)
        || path.split('/').any(|p| p == "." || p == "..")
    {
        return Err(bad("请输入以 / 开头的绝对路径，不能包含 . 或 .. 路径段"));
    }
    Ok(())
}
fn valid_name(name: &str) -> Result<()> {
    if name.len() > 255
        || !crate::utils::is_valid_path_component(name)
        || name.chars().any(char::is_control)
    {
        return Err(bad("文件名无效，不能包含路径分隔符"));
    }
    Ok(())
}

async fn action(
    State(state): State<Arc<WebState>>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Json(input): Json<Action>,
) -> Result<Json<Value>> {
    state.authorize(&headers).await?;
    let _permit = state
        .operations
        .try_acquire()
        .map_err(|_| err(StatusCode::TOO_MANY_REQUESTS, "操作过多，请稍后重试"))?;
    if matches!(
        input.action.as_str(),
        "remove" | "mkdir" | "download" | "refresh" | "settings"
    ) {
        state.secondary(peer.ip(), input.secondary_password).await?;
    }
    let client = &state.ctx.openlist;
    let result = match input.action.as_str() {
        "storages" => {
            json!({"kind": "storages", "items": client.storage_list().await.map_err(upstream)?})
        }
        "browse" => {
            valid_path(&input.path)?;
            json!({"kind": "files", "path": input.path, "items": client.fs_list(&input.path).await.map_err(upstream)?})
        }
        "file" => {
            valid_path(&input.path)?;
            let file = client.fs_get(&input.path).await.map_err(upstream)?;
            json!({"kind": "file", "name": file.name, "size": file.size, "url": file.raw_url})
        }
        "search" => {
            if input.keyword.trim().is_empty() || input.keyword.len() > 512 {
                return Err(bad("请输入 1–512 字节的搜索关键词"));
            }
            let allowed = state.ctx.config.read().await.search.allowed_sources.clone();
            let pansou_enabled = state.ctx.config.read().await.pansou.is_some();
            let sukebei_enabled = allowed.iter().any(|s| s == "sukebei");
            if !pansou_enabled && !sukebei_enabled {
                return Err(bad("请先配置 PanSou 或启用可用的搜索源"));
            }
            let (pansou, sukebei) = tokio::join!(
                async {
                    if pansou_enabled {
                        state.ctx.pansou.search(&input.keyword).await
                    } else {
                        Ok(vec![])
                    }
                },
                async {
                    if sukebei_enabled {
                        state.ctx.pansou.search_sukebei(&input.keyword).await
                    } else {
                        Ok(vec![])
                    }
                }
            );
            let mut warnings = Vec::new();
            if pansou.is_err() {
                warnings.push("PanSou 暂时不可用");
            }
            if sukebei.is_err() {
                warnings.push("Sukebei 暂时不可用");
            }
            if (!pansou_enabled || pansou.is_err()) && (!sukebei_enabled || sukebei.is_err()) {
                return Err(upstream("search unavailable"));
            }
            let mut items = pansou.unwrap_or_default();
            items.retain(|r| allowed.contains(&r.pan_type));
            items.extend(sukebei.unwrap_or_default());
            json!({"kind": "search", "items": items, "warnings": warnings})
        }
        "tasks" => {
            let (undone, done) = tokio::try_join!(
                client.get_offline_download_undone_task(),
                client.get_offline_download_done_task()
            )
            .map_err(upstream)?;
            json!({"kind": "tasks", "undone": undone, "done": done})
        }
        "tools" => {
            json!({"kind": "tools", "items": client.get_offline_download_tools().await.map_err(upstream)?})
        }
        "download" => {
            valid_path(&input.path)?;
            if input.urls.is_empty() || input.urls.len() > 20 || input.tool.trim().is_empty() {
                return Err(bad("请选择下载工具、目录，并输入 1–20 个链接"));
            }
            for link in &input.urls {
                let url = url::Url::parse(link).map_err(|_| bad("下载链接格式无效"))?;
                if !matches!(url.scheme(), "http" | "https" | "magnet" | "ed2k") {
                    return Err(bad("仅支持 HTTP、HTTPS、magnet 和 ed2k 链接"));
                }
            }
            client
                .add_offline_download(input.urls, &input.tool, &input.path)
                .await
                .map_err(upstream)?;
            json!({"kind": "message", "text": "下载任务已提交，可在任务列表查看进度。"})
        }
        "refresh" => {
            valid_path(&input.path)?;
            client
                .fs_list_refresh(&input.path)
                .await
                .map_err(upstream)?;
            json!({"kind": "message", "text": format!("缓存已刷新：{}", input.path)})
        }
        "mkdir" => {
            valid_path(&input.path)?;
            valid_name(&input.name)?;
            client
                .fs_mkdir(&format!(
                    "{}/{}",
                    input.path.trim_end_matches('/'),
                    input.name
                ))
                .await
                .map_err(upstream)?;
            json!({"kind": "message", "text": "文件夹已创建。"})
        }
        "remove" => {
            valid_path(&input.path)?;
            valid_name(&input.name)?;
            client
                .fs_remove(&input.path, vec![input.name])
                .await
                .map_err(upstream)?;
            json!({"kind": "message", "text": "删除操作已完成。"})
        }
        "settings" => {
            valid_path(&input.path)?;
            if input.tool.trim().is_empty()
                || input.tool.len() > 128
                || input.allowed_sources.len() > 50
                || input.allowed_sources.iter().any(|s| s.len() > 64)
            {
                return Err(bad("下载工具或搜索源配置无效"));
            }
            state
                .ctx
                .config
                .write()
                .await
                .update_and_save(|cfg| {
                    cfg.openlist.download_path = input.path;
                    cfg.openlist.download_tool = input.tool;
                    cfg.search.allowed_sources = input.allowed_sources;
                })
                .map_err(|_| {
                    err(
                        StatusCode::INTERNAL_SERVER_ERROR,
                        "配置保存失败，请检查 config.yaml 的写入权限",
                    )
                })?;
            json!({"kind": "message", "text": "设置已保存。"})
        }
        _ => return Err(bad("不支持的操作")),
    };
    Ok(Json(result))
}

#[derive(Deserialize)]
struct UploadQuery {
    path: String,
    name: String,
}
async fn upload(
    State(state): State<Arc<WebState>>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Query(input): Query<UploadQuery>,
    body: Bytes,
) -> Result<Json<Value>> {
    state.authorize(&headers).await?;
    let _permit = state
        .operations
        .try_acquire()
        .map_err(|_| err(StatusCode::TOO_MANY_REQUESTS, "操作过多，请稍后重试"))?;
    valid_path(&input.path)?;
    valid_name(&input.name)?;
    // Base64 transports Unicode passwords safely in HTTP headers; never in a URL.
    let password = headers
        .get("x-secondary-password")
        .map(|v| {
            use base64::Engine;
            let bytes = base64::engine::general_purpose::STANDARD
                .decode(v.as_bytes())
                .map_err(|_| bad("二级密码编码无效"))?;
            String::from_utf8(bytes).map_err(|_| bad("二级密码编码无效"))
        })
        .transpose()?;
    state.secondary(peer.ip(), password).await?;
    // Serialize the existence check and write within this server. Refresh the
    // directory so a previous upload cannot be hidden by OpenList's list cache.
    let _upload = state.upload_lock.lock().await;
    state.authorize(&headers).await?;
    let existing = state
        .ctx
        .openlist
        .fs_list_refresh(&input.path)
        .await
        .map_err(upstream)?;
    if existing.iter().any(|f| f.name == input.name) {
        return Err(err(StatusCode::CONFLICT, "目标文件已存在，请重命名后上传"));
    }
    state
        .ctx
        .openlist
        .fs_put_bytes(body.to_vec(), &input.path, &input.name)
        .await
        .map_err(upstream)?;
    Ok(Json(json!({"kind": "message", "text": "文件已上传。"})))
}
