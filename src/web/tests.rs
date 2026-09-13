use super::*;
use crate::{
    api::{openlist::OpenListClient, pansou::PanSouClient},
    config::{Config, OpenListConfig, PanSouConfig, SearchConfig, UserConfig},
    PathRegistry,
};
use axum::{
    body::{to_bytes, Body},
    http::Request as HttpRequest,
    routing::any,
};
use std::{
    collections::{HashSet, VecDeque},
    sync::{
        atomic::{AtomicBool, AtomicUsize, Ordering},
        OnceLock,
    },
};
use tokio::sync::RwLock;
use tower::ServiceExt;

const PASSWORD: &str = "test-login-password";
const SECONDARY: &str = "test-secondary-password";

fn hash(password: &str) -> String {
    Argon2::default()
        .hash_password(password.as_bytes(), &SaltString::generate(&mut OsRng))
        .unwrap()
        .to_string()
}
fn web_config() -> WebConfig {
    static HASHES: OnceLock<(String, String)> = OnceLock::new();
    let hashes = HASHES.get_or_init(|| (hash(PASSWORD), hash(SECONDARY)));
    WebConfig {
        username: "admin".into(),
        password: None,
        password_hash: hashes.0.clone(),
        secondary_password_hash: Some(hashes.1.clone()),
        bind: "127.0.0.1:0".into(),
        cookie_secure: false,
    }
}

async fn fixture() -> (Arc<WebState>, Arc<AtomicUsize>) {
    let writes = Arc::new(AtomicUsize::new(0));
    let capture = writes.clone();
    let uploaded = Arc::new(AtomicBool::new(false));
    let mock = Router::new().fallback(any(move |request: Request| {
        let writes = capture.clone();
        let uploaded = uploaded.clone();
        async move {
            let path = request.uri().path().to_string();
            let concurrent_upload = request.headers().get("file-path").and_then(|v| v.to_str().ok()).is_some_and(|v| v.contains("concurrent"));
            if path == "/api/search" {
                return Json(json!({"code":200,"data":{"merged_by_type":{
                    "baidu":[{"note":"设计资料合集","url":"https://example.com/resource","password":"abcd"}],
                    "magnet":[{"note":"开源镜像","url":"magnet:?xt=urn:btih:123"}],
                    "quark":[{"note":"应当被过滤","url":"https://example.com/filtered"}]
                }}}));
            }
            assert_eq!(request.headers().get("authorization").unwrap(), "mock-openlist-token");
            let body = to_bytes(request.into_body(), UPLOAD_LIMIT).await.unwrap();
            let input: Value = serde_json::from_slice(&body).unwrap_or(Value::Null);
            let data = match path.as_str() {
                "/api/admin/storage/list" => json!({"content":[{"id":1,"mount_path":"/资料","remark":"个人资料库"},{"id":2,"mount_path":"/归档","remark":"长期存储"}]}),
                "/api/fs/list" => {
                    if input["path"] == "/concurrent" {
                        // Deliberately emulate a stale cache unless refresh is requested.
                        if input["refresh"] == true && uploaded.load(Ordering::SeqCst) {
                            json!({"content":[{"name":"same.txt","is_dir":false,"size":4,"type":0}]})
                        } else { json!({"content":[]}) }
                    }
                    else if input["path"] == "/empty" { json!({"content":[]}) }
                    else if input["path"] == "/broken" { return Json(json!({"code":500,"message":"secret mock-openlist-token must not leak"})); }
                    else { json!({"content":[
                        {"name":"设计参考","is_dir":true,"size":0,"type":1},
                        {"name":"项目说明.pdf","is_dir":false,"size":2512345,"type":0},
                        {"name":"<img src=x onerror=alert(1)>.txt","is_dir":false,"size":38,"type":0}
                    ]}) }
                }
                "/api/fs/get" => json!({"name":"项目说明.pdf","is_dir":false,"size":2512345,"type":0,"raw_url":"https://example.com/demo.pdf"}),
                "/api/public/offline_download_tools" => json!(["qbittorrent", "aria2"]),
                "/api/admin/task/offline_download/undone" => json!([{"id":"task-1","name":"参考资料.zip","status":"下载中","progress":42.5}]),
                "/api/admin/task/offline_download/done" => json!([{"id":"task-2","name":"归档资料.zip","status":"完成","progress":100}]),
                "/api/fs/remove" | "/api/fs/mkdir" | "/api/fs/put" | "/api/fs/add_offline_download" => {
                    if concurrent_upload {
                        tokio::time::sleep(Duration::from_millis(60)).await;
                        uploaded.store(true, Ordering::SeqCst);
                    }
                    writes.fetch_add(1, Ordering::SeqCst); Value::Null
                }
                _ => Value::Null,
            };
            Json(json!({"code":200,"message":"success","data":data}))
        }
    }));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let host = format!("http://{}", listener.local_addr().unwrap());
    tokio::spawn(async move {
        axum::serve(listener, mock).await.unwrap();
    });
    let config = Config {
        log_level: "INFO".into(),
        user: UserConfig::default(),
        web: Some(web_config()),
        openlist: OpenListConfig {
            openlist_host: host.clone(),
            openlist_token: "mock-openlist-token".into(),
            download_path: "/资料".into(),
            download_tool: "qbittorrent".into(),
        },
        pansou: Some(PanSouConfig {
            pansou_host: host,
            pansou_token: None,
        }),
        proxy: None,
        search: SearchConfig {
            allowed_sources: vec!["baidu".into(), "magnet".into()],
        },
    };
    let client = reqwest::Client::new();
    let ctx = Arc::new(BotContext {
        openlist: OpenListClient::new(&config, client.clone()),
        pansou: PanSouClient::new(&config, client.clone()),
        http_client: client,
        config: Arc::new(RwLock::new(config)),
        user_states: Mutex::new(HashMap::new()),
        path_registry: Mutex::new(PathRegistry {
            map: HashMap::new(),
            counter: 0,
        }),
        pansou_pages: Mutex::new(HashMap::new()),
        pansou_results: Mutex::new(HashMap::new()),
        pansou_order: Mutex::new(VecDeque::new()),
        od_done_ids: Mutex::new(HashSet::new()),
    });
    (
        Arc::new(WebState {
            ctx,
            config: web_config(),
            sessions: Mutex::new(HashMap::new()),
            attempts: Mutex::new(HashMap::new()),
            hashing: Arc::new(Semaphore::new(2)),
            operations: Semaphore::new(16),
            upload_lock: Mutex::new(()),
        }),
        writes,
    )
}

async fn request(
    app: &Router,
    path: &str,
    body: Value,
    cookie: Option<&str>,
    csrf: bool,
) -> Response {
    let mut builder = HttpRequest::builder()
        .method("POST")
        .uri(path)
        .header("content-type", "application/json");
    if csrf {
        builder = builder.header("x-openlist-web", "1");
    }
    if let Some(cookie) = cookie {
        builder = builder.header("cookie", cookie);
    }
    app.clone()
        .oneshot(
            builder
                .extension(ConnectInfo(
                    "127.0.0.1:12345".parse::<SocketAddr>().unwrap(),
                ))
                .body(Body::from(body.to_string()))
                .unwrap(),
        )
        .await
        .unwrap()
}
async fn json_body(response: Response) -> Value {
    serde_json::from_slice(
        &to_bytes(response.into_body(), UPLOAD_LIMIT + 1)
            .await
            .unwrap(),
    )
    .unwrap()
}
async fn sign_in(app: &Router) -> String {
    let response = request(
        app,
        "/api/login",
        json!({"username":"admin","password":PASSWORD}),
        None,
        true,
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
    let cookie = response
        .headers()
        .get("set-cookie")
        .unwrap()
        .to_str()
        .unwrap();
    assert!(cookie.contains("HttpOnly; SameSite=Strict"));
    cookie.split(';').next().unwrap().to_string()
}

#[tokio::test]
async fn authentication_csrf_expiry_logout_and_rate_limit() {
    let (state, _) = fixture().await;
    let app = routes(state.clone());
    assert_eq!(
        request(
            &app,
            "/api/action",
            json!({"action":"storages"}),
            None,
            true
        )
        .await
        .status(),
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        request(
            &app,
            "/api/login",
            json!({"username":"admin","password":PASSWORD}),
            None,
            false
        )
        .await
        .status(),
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        request(
            &app,
            "/api/login",
            json!({"username":"admin","password":"bad"}),
            None,
            true
        )
        .await
        .status(),
        StatusCode::UNAUTHORIZED
    );
    let cookie = sign_in(&app).await;
    assert_eq!(
        request(&app, "/api/session", json!({}), Some(&cookie), false)
            .await
            .status(),
        StatusCode::FORBIDDEN
    );
    let session =
        json_body(request(&app, "/api/session", json!({}), Some(&cookie), true).await).await;
    assert_eq!(session["secondary_required"], true);
    assert!(!session.to_string().contains("password_hash"));
    assert!(!session.to_string().contains("mock-openlist-token"));
    request(&app, "/api/logout", json!({}), Some(&cookie), true).await;
    assert_eq!(
        request(&app, "/api/session", json!({}), Some(&cookie), true)
            .await
            .status(),
        StatusCode::UNAUTHORIZED
    );
    let cookie = sign_in(&app).await;
    for session in state.sessions.lock().await.values_mut() {
        session.expires = Instant::now() - Duration::from_secs(1);
    }
    assert_eq!(
        request(&app, "/api/session", json!({}), Some(&cookie), true)
            .await
            .status(),
        StatusCode::UNAUTHORIZED
    );
    state.attempts.lock().await.insert(
        "127.0.0.1".parse().unwrap(),
        Attempt {
            since: Instant::now(),
            count: 10,
        },
    );
    assert_eq!(
        request(
            &app,
            "/api/login",
            json!({"username":"admin","password":PASSWORD}),
            None,
            true
        )
        .await
        .status(),
        StatusCode::TOO_MANY_REQUESTS
    );
}

#[tokio::test]
async fn protected_mutations_paths_upload_conflicts_and_upstream_errors() {
    let (state, writes) = fixture().await;
    let app = routes(state.clone());
    let cookie = sign_in(&app).await;
    for action in ["remove", "mkdir", "download", "refresh", "settings"] {
        assert_eq!(
            request(
                &app,
                "/api/action",
                json!({"action":action,"path":"/资料","name":"file"}),
                Some(&cookie),
                true
            )
            .await
            .status(),
            StatusCode::FORBIDDEN
        );
    }
    assert_eq!(
        request(
            &app,
            "/api/action",
            json!({"action":"remove","path":"/资料","name":"file","secondary_password":"wrong"}),
            Some(&cookie),
            true
        )
        .await
        .status(),
        StatusCode::FORBIDDEN
    );
    assert_eq!(writes.load(Ordering::SeqCst), 0);
    assert_eq!(
        request(
            &app,
            "/api/action",
            json!({"action":"remove","path":"/资料","name":"file","secondary_password":SECONDARY}),
            Some(&cookie),
            true
        )
        .await
        .status(),
        StatusCode::OK
    );
    assert_eq!(writes.load(Ordering::SeqCst), 1);
    assert_eq!(
        request(
            &app,
            "/api/action",
            json!({"action":"browse","path":"/a/../b"}),
            Some(&cookie),
            true
        )
        .await
        .status(),
        StatusCode::BAD_REQUEST
    );
    let response = json_body(
        request(
            &app,
            "/api/action",
            json!({"action":"browse","path":"/broken"}),
            Some(&cookie),
            true,
        )
        .await,
    )
    .await;
    assert!(!response.to_string().contains("mock-openlist-token"));
    use base64::Engine;
    for (path, name, expected) in [
        ("/资料", "项目说明.pdf", StatusCode::CONFLICT),
        ("/empty", "new.txt", StatusCode::OK),
    ] {
        let url = format!(
            "/api/upload?{}",
            url::form_urlencoded::Serializer::new(String::new())
                .append_pair("path", path)
                .append_pair("name", name)
                .finish()
        );
        let response = app
            .clone()
            .oneshot(
                HttpRequest::builder()
                    .method("POST")
                    .uri(url)
                    .header("cookie", &cookie)
                    .header("x-openlist-web", "1")
                    .header(
                        "x-secondary-password",
                        base64::engine::general_purpose::STANDARD.encode(SECONDARY),
                    )
                    .extension(ConnectInfo(
                        "127.0.0.1:12345".parse::<SocketAddr>().unwrap(),
                    ))
                    .body(Body::from("test bytes"))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), expected);
    }
    assert_eq!(writes.load(Ordering::SeqCst), 2);
}

#[tokio::test]
async fn assets_have_security_headers_and_upload_auth_precedes_body() {
    let (state, _) = fixture().await;
    let app = routes(state);
    for path in ["/", "/app.js", "/app.css"] {
        let response = app
            .clone()
            .oneshot(
                HttpRequest::builder()
                    .uri(path)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(response.headers()["cache-control"], "no-store");
        assert!(response.headers()["content-security-policy"]
            .to_str()
            .unwrap()
            .contains("frame-ancestors 'none'"));
    }
    assert_eq!(
        request(&app, "/api/upload?path=/&name=a", json!({}), None, true)
            .await
            .status(),
        StatusCode::UNAUTHORIZED
    );
}

#[tokio::test]
async fn search_filters_sources_and_reads_return_structured_data() {
    let (state, _) = fixture().await;
    let app = routes(state);
    let cookie = sign_in(&app).await;
    let response = json_body(
        request(
            &app,
            "/api/action",
            json!({"action":"search","keyword":"资料"}),
            Some(&cookie),
            true,
        )
        .await,
    )
    .await;
    assert_eq!(response["items"].as_array().unwrap().len(), 2);
    assert!(!response.to_string().contains("应当被过滤"));
    for (action, kind) in [
        ("storages", "storages"),
        ("browse", "files"),
        ("file", "file"),
        ("tasks", "tasks"),
        ("tools", "tools"),
    ] {
        let response = json_body(
            request(
                &app,
                "/api/action",
                json!({"action":action,"path":"/资料"}),
                Some(&cookie),
                true,
            )
            .await,
        )
        .await;
        assert_eq!(response["kind"], kind);
    }
}

#[tokio::test]
async fn secondary_can_be_disabled_and_secure_cookies_are_configurable() {
    let (mut state, writes) = fixture().await;
    Arc::get_mut(&mut state)
        .unwrap()
        .config
        .secondary_password_hash = None;
    Arc::get_mut(&mut state).unwrap().config.cookie_secure = true;
    let app = routes(state);
    let cookie = sign_in(&app).await;
    let response = request(
        &app,
        "/api/action",
        json!({"action":"mkdir","path":"/资料","name":"new"}),
        Some(&cookie),
        true,
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(writes.load(Ordering::SeqCst), 1);
    assert!(super::cookie(
        &WebConfig {
            cookie_secure: true,
            ..web_config()
        },
        "token",
        10
    )
    .to_str()
    .unwrap()
    .ends_with("; Secure"));
}

#[test]
fn reject_unsafe_paths_and_invalid_password_config() {
    for path in ["", "relative", "/../a", "/a/./b", "/a\\b", "/a\nb"] {
        assert!(valid_path(path).is_err());
    }
    for name in ["", ".", "..", "a/b", "a\\b", "a\nb"] {
        assert!(valid_name(name).is_err());
    }
    assert!(valid_path("/资料/2026 年").is_ok());
    let mut config = web_config();
    assert!(validate_config(&config).is_ok());
    config.password_hash = "plaintext".into();
    assert!(validate_config(&config).is_err());
    config.password = Some(PASSWORD.into());
    assert!(validate_config(&config).is_err());
    config.password_hash.clear();
    assert!(validate_config(&config).is_ok());
    let serialized = serde_yaml::to_string(&config).unwrap();
    assert!(!serialized
        .lines()
        .any(|line| line.starts_with("password_hash:")));
    let restored: WebConfig = serde_yaml::from_str(&serialized).unwrap();
    assert_eq!(restored.password.as_deref(), Some(PASSWORD));
    config.password = Some(String::new());
    assert!(validate_config(&config).is_err());
    config.password = Some("x".repeat(1025));
    assert!(validate_config(&config).is_err());
    config.password = None;
    assert!(validate_config(&config).is_err());
}

#[tokio::test]
async fn plaintext_login_accepts_only_correct_credentials() {
    let (mut state, _) = fixture().await;
    let config = &mut Arc::get_mut(&mut state).unwrap().config;
    config.password = Some(PASSWORD.into());
    config.password_hash.clear();
    let app = routes(state);
    sign_in(&app).await;
    for (username, password) in [("admin", "wrong"), ("wrong", PASSWORD)] {
        let response = request(
            &app,
            "/api/login",
            json!({"username":username,"password":password}),
            None,
            true,
        )
        .await;
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    }
}

#[tokio::test]
async fn concurrent_same_name_uploads_write_once_even_with_stale_list_cache() {
    let (state, writes) = fixture().await;
    let app = routes(state);
    let cookie = sign_in(&app).await;
    let upload = || {
        use base64::Engine;
        app.clone().oneshot(
            HttpRequest::builder()
                .method("POST")
                .uri("/api/upload?path=/concurrent&name=same.txt")
                .header("cookie", &cookie)
                .header("x-openlist-web", "1")
                .header(
                    "x-secondary-password",
                    base64::engine::general_purpose::STANDARD.encode(SECONDARY),
                )
                .extension(ConnectInfo(
                    "127.0.0.1:12345".parse::<SocketAddr>().unwrap(),
                ))
                .body(Body::from("test"))
                .unwrap(),
        )
    };
    let (first, second) = tokio::join!(upload(), upload());
    let mut statuses = [
        first.unwrap().status().as_u16(),
        second.unwrap().status().as_u16(),
    ];
    statuses.sort();
    assert_eq!(statuses, [200, 409]);
    assert_eq!(writes.load(Ordering::SeqCst), 1);
}

/// Local UI fixture, never connects to a real OpenList or Telegram account.
/// cargo test web::tests::browser_fixture -- --ignored --nocapture
#[tokio::test]
#[ignore]
async fn browser_fixture() {
    let (state, _) = fixture().await;
    let listener = tokio::net::TcpListener::bind("127.0.0.1:18080")
        .await
        .unwrap();
    println!(
        "Browser fixture: http://127.0.0.1:18080 (admin / {PASSWORD}; secondary: {SECONDARY})"
    );
    axum::serve(
        listener,
        routes(state).into_make_service_with_connect_info::<SocketAddr>(),
    )
    .await
    .unwrap();
}
