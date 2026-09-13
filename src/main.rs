mod api;
mod config;
mod handlers;
mod utils;
mod web;

use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::Arc;
use teloxide::net::Download;
use teloxide::prelude::*;
use teloxide::types::{
    BotCommand, BotCommandScope, ChatId, InlineKeyboardButton, InlineKeyboardMarkup, MessageId,
};
use teloxide::utils::command::BotCommands;
use tokio::sync::{Mutex, RwLock};
use tracing::{error, info};
use tracing_subscriber::EnvFilter;

use crate::api::openlist::{FileItem, OpenListClient};
use crate::api::pansou::{PanSouClient, PanSouResult};
use crate::config::Config;
use crate::handlers::config_download::{build_config_edit_menu, CONFIG_ITEMS};
use crate::handlers::download::build_download_confirm_message;
use crate::handlers::file_refresh::{handle_refresh, run_refresh_openlist};
use crate::handlers::help::{handle_help, handle_start};
use crate::handlers::offline_download::{
    extract_file_info, handle_download, handle_ods, start_background_notifier,
    start_od_download_flow,
};
use crate::handlers::search::{handle_s, PanSouPage};
use crate::handlers::storage_browse::{build_file_list, handle_st};
use crate::utils::{
    escape_code, escape_link_url, format_size, is_admin, is_member, is_valid_path_component,
    md_escape, parent_path, path_is_direct_child, path_is_within,
};

#[derive(Clone, Debug, PartialEq)]
pub enum StorageOpState {
    None,
    AwaitingMkdir,
    AwaitingUpload,
}

#[derive(Clone, Debug)]
pub enum OdStep {
    SelectingTool,
    BrowsingPath,
    EnteringUrl,
    Confirming,
}

#[derive(Clone, Debug)]
pub enum CfStep {
    SelectingTool,
    BrowsingPath,
    BrowsingDir,
    EnteringUrl,
    EditConfig,
}

#[derive(Clone, Debug)]
pub enum UserState {
    None,
    StorageBrowse {
        storage_id: String,
        root_path: String,
        current_path: String,
        browse_msg_id: Option<MessageId>,
        files: Vec<FileItem>,
        page: usize,
        op_state: StorageOpState,
        prompt_msg_id: Option<MessageId>,
        // Path plus the confirmation message that owns it. This prevents an
        // old confirmation button from acting on a newer pending deletion.
        pending_delete_path: Option<(String, MessageId)>,
    },
    OfflineDownload {
        step: OdStep,
        message_id: Option<MessageId>,
        tool: Option<String>,
        path: Option<String>,
        urls: Vec<String>,
    },
    ConfigEdit {
        step: CfStep,
        message_id: Option<MessageId>,
        tool: Option<String>,
        path: Option<String>,
        config_key: Option<String>,
        config_name: Option<String>,
    },
    TorrentDownloadSelect {
        index: usize,
        cmid: String,
        path: String,
        tool: String,
        url: String,
        file_name: String,
        current_path: Option<String>,
    },
    AwaitingSearchQuery {
        prompt_msg_id: Option<MessageId>,
        // The user who opened the search prompt. State is keyed by chat_id, so
        // in a group we must only consume the *originator's* next message —
        // otherwise the bot would eat (and delete) an unrelated user's message.
        user_id: i64,
    },
}

pub struct PathRegistry {
    pub map: HashMap<String, String>,
    pub counter: usize,
}

pub struct BotContext {
    pub config: Arc<RwLock<Config>>,
    pub openlist: OpenListClient,
    pub pansou: PanSouClient,
    pub http_client: reqwest::Client,

    // States and registries
    // Interactive state must be isolated per user. A chat-only key lets any
    // member of a group overwrite another member's (or the admin's) flow.
    pub user_states: Mutex<HashMap<(ChatId, i64), UserState>>,
    pub path_registry: Mutex<PathRegistry>,

    // Callback & pagination caches
    pub pansou_pages: Mutex<HashMap<String, PanSouPage>>,
    pub pansou_results: Mutex<HashMap<String, PanSouResult>>,
    // Insertion order of search sessions (cmid), for bounded eviction.
    pub pansou_order: Mutex<VecDeque<String>>,

    pub od_done_ids: Mutex<HashSet<String>>,
}

/// Maximum number of search sessions kept in the PanSou caches.
const MAX_PANSOU_SESSIONS: usize = 50;

/// Record a search session and evict the oldest ones so the PanSou caches
/// stay bounded for a long-running bot.
pub async fn remember_pansou_session(ctx: &BotContext, cmid: &str) {
    let mut order = ctx.pansou_order.lock().await;
    order.retain(|c| c != cmid);
    order.push_back(cmid.to_string());
    while order.len() > MAX_PANSOU_SESSIONS {
        if let Some(old) = order.pop_front() {
            ctx.pansou_pages.lock().await.remove(&old);
            let prefix = format!("{}_", old);
            ctx.pansou_results
                .lock()
                .await
                .retain(|k, _| !k.starts_with(&prefix));
        }
    }
}

pub async fn register_path(ctx: &BotContext, path: &str) -> String {
    let mut reg = ctx.path_registry.lock().await;
    reg.counter += 1;
    let key = reg.counter.to_string();
    reg.map.insert(key.clone(), path.to_string());
    // Bound memory: keep only the most recent registrations so long-running
    // sessions don't grow the registry without limit.
    if reg.map.len() > 5000 {
        let cutoff = reg.counter.saturating_sub(5000);
        reg.map
            .retain(|k, _| k.parse::<usize>().map_or(true, |n| n > cutoff));
    }
    key
}

pub async fn get_path(ctx: &BotContext, key: &str) -> Option<String> {
    let reg = ctx.path_registry.lock().await;
    reg.map.get(key).cloned()
}

#[derive(BotCommands, Clone)]
#[command(
    rename_rule = "lowercase",
    description = "These commands are supported:"
)]
pub enum Command {
    #[command(description = "显示欢迎信息.")]
    Start,
    #[command(description = "显示详细命令帮助.")]
    Help,
    #[command(description = "搜索网盘文件")]
    Search(String),
    #[command(description = "浏览 OpenList 存储文件")]
    Browse,
    #[command(description = "离线下载与设置")]
    Download,
    #[command(description = "刷新缓存")]
    Refresh,
    #[command(description = "刷新机器人命令菜单.")]
    Menu,
    #[command(description = "开启/关闭代理（重启后生效）.")]
    Px,
}

fn build_client(config: &Config) -> Result<reqwest::Client, reqwest::Error> {
    let mut builder = reqwest::Client::builder().timeout(Duration::from_secs(30));
    if let Some(proxy_cfg) = &config.proxy {
        if proxy_cfg.enable {
            let proxy_url = format!(
                "{}://{}:{}",
                proxy_cfg.scheme, proxy_cfg.hostname, proxy_cfg.port
            );
            if let Ok(proxy) = reqwest::Proxy::all(&proxy_url) {
                builder = builder.proxy(proxy);
                info!("Using SOCKS5/HTTP Proxy: {}", proxy_url);
            }
        }
    }
    builder.build()
}

fn bot_commands() -> Vec<BotCommand> {
    vec![
        BotCommand::new("start", "开始使用"),
        BotCommand::new("search", "搜索网盘文件"),
        BotCommand::new("browse", "浏览存储"),
        BotCommand::new("download", "离线下载与设置"),
        BotCommand::new("refresh", "刷新缓存"),
        BotCommand::new("help", "查看帮助"),
    ]
}

async fn register_bot_commands(bot: &Bot) {
    if let Err(e) = bot
        .set_my_commands(bot_commands())
        .scope(BotCommandScope::AllPrivateChats)
        .await
    {
        error!("Failed to register bot commands (private): {}", e);
    } else {
        info!("Bot commands registered for private chats");
    }

    if let Err(e) = bot
        .set_my_commands(bot_commands())
        .scope(BotCommandScope::AllGroupChats)
        .await
    {
        error!("Failed to register bot commands (group): {}", e);
    } else {
        info!("Bot commands registered for group chats");
    }
}

async fn menu_command(bot: &Bot, msg: &Message, ctx: &BotContext) -> ResponseResult<()> {
    let user_id = msg.from().map_or(0, |u| u.id.0 as i64);
    let cfg = ctx.config.read().await;
    if !is_admin(user_id, &cfg) {
        return Ok(());
    }
    drop(cfg);

    register_bot_commands(bot).await;
    bot.send_message(
        msg.chat.id,
        "菜单设置成功，请退出聊天界面重新进入来刷新菜单",
    )
    .await?;
    Ok(())
}

async fn toggle_proxy_command(bot: &Bot, msg: &Message, ctx: &BotContext) -> ResponseResult<()> {
    let user_id = msg.from().map_or(0, |u| u.id.0 as i64);
    // Some(status) = toggled; None = missing proxy section.
    // The write lock must not be held across the Telegram API await below.
    let result: Result<Option<&'static str>, String> = {
        let mut cfg = ctx.config.write().await;
        if !is_admin(user_id, &cfg) {
            return Ok(());
        }
        match cfg.proxy.as_ref() {
            Some(proxy_cfg) => {
                let enabled = !proxy_cfg.enable;
                cfg.update_and_save(|candidate| {
                    if let Some(proxy) = &mut candidate.proxy {
                        proxy.enable = enabled;
                    }
                })
                .map(|_| Some(if enabled { "开启" } else { "关闭" }))
                .map_err(|e| e.to_string())
            }
            None => Ok(None),
        }
    };

    match result {
        Ok(Some(status)) => {
            bot.send_message(msg.chat.id, format!("代理已{}，重启机器人后生效", status))
                .await?;
        }
        Ok(None) => {
            bot.send_message(msg.chat.id, "配置中缺少 proxy 字段，无法切换")
                .await?;
        }
        Err(e) => {
            bot.send_message(msg.chat.id, format!("代理配置保存失败: {}", e))
                .await?;
        }
    }
    Ok(())
}

use std::time::Duration;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    if std::env::args().nth(1).as_deref() == Some("hash-password") {
        return web::hash_password_command();
    }
    let config = Config::load()?;
    if let Some(web) = &config.web {
        web::validate_config(web)?;
    }
    let telegram_enabled = !config.user.bot_token.trim().is_empty();
    if !telegram_enabled && config.web.is_none() {
        return Err("Configure web access or a Telegram bot token".into());
    }

    // Logging setup
    let log_filter =
        EnvFilter::try_new(&config.log_level).unwrap_or_else(|_| EnvFilter::new("INFO"));
    tracing_subscriber::fmt().with_env_filter(log_filter).init();

    info!("========== Starting Rust OpenList Bot ==========");

    let bot_token = config.user.bot_token.clone();

    let http_client = build_client(&config)?;
    let openlist = OpenListClient::new(&config, http_client.clone());
    let pansou = PanSouClient::new(&config, http_client.clone());

    // Check OpenList login
    if let Err(e) = openlist.login().await {
        error!("Failed to connect to OpenList: {}", e);
    } else {
        info!("OpenList login check successful");
    }

    let bot_context = Arc::new(BotContext {
        config: Arc::new(RwLock::new(config)),
        openlist,
        pansou,
        http_client,
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

    let web_server = web::start(bot_context.clone()).await?;
    if !telegram_enabled {
        let server = web_server.expect("web configured for web-only mode");
        tokio::select! {
            result = server => { result??; }
            _ = tokio::signal::ctrl_c() => {}
        }
        return Ok(());
    }
    let bot = Bot::new(bot_token);

    // Register Bot commands on Telegram
    register_bot_commands(&bot).await;

    // Send startup message to admin
    let admin_chat_id = {
        let cfg = bot_context.config.read().await;
        cfg.user.admin
    };
    if admin_chat_id != 0 {
        let startup_msg = "✅ Bot (Rust重构版) 已启动!\n\n\
                           📚 可用命令:\n\
                           /search <关键词> - 搜索网盘\n\
                           /browse - 浏览存储\n\
                           /download - 离线下载与设置\n\
                           /refresh - 刷新缓存\n\
                           /help - 帮助";
        let _ = bot.send_message(ChatId(admin_chat_id), startup_msg).await;
    }

    // Start background task completion checking loop
    let bot_clone = bot.clone();
    let ctx_clone = bot_context.clone();
    tokio::spawn(async move {
        start_background_notifier(bot_clone, ctx_clone).await;
    });

    // Create the dispatcher and add handlers
    let handler = dptree::entry()
        .branch(
            Update::filter_message()
                .branch(
                    dptree::entry()
                        .filter_command::<Command>()
                        .endpoint(command_handler),
                )
                .branch(dptree::entry().endpoint(text_message_handler)),
        )
        .branch(Update::filter_callback_query().endpoint(callback_handler));

    // Spawn explicit Ctrl+C handler to guarantee instant exit
    tokio::spawn(async move {
        tokio::signal::ctrl_c().await.ok();
        info!("Received Ctrl+C signal. Exiting process immediately...");
        std::process::exit(0);
    });

    info!("Bot is listening...");
    let mut dispatcher = Dispatcher::builder(bot, handler)
        .dependencies(dptree::deps![bot_context])
        .build();
    if let Some(server) = web_server {
        tokio::select! {
            result = server => { result??; }
            _ = dispatcher.dispatch() => {}
        }
    } else {
        dispatcher.dispatch().await;
    }

    Ok(())
}

async fn command_handler(
    bot: Bot,
    msg: Message,
    cmd: Command,
    ctx: Arc<BotContext>,
) -> ResponseResult<()> {
    match cmd {
        Command::Start => handle_start(bot, msg).await?,
        Command::Help => handle_help(bot, msg).await?,
        Command::Search(query) => handle_s(bot, msg, query, ctx).await?,
        Command::Browse => handle_st(bot, msg, ctx).await?,
        Command::Download => handle_download(bot, msg, ctx).await?,
        Command::Refresh => handle_refresh(bot, msg, ctx).await?,
        Command::Menu => menu_command(&bot, &msg, &ctx).await?,
        Command::Px => toggle_proxy_command(&bot, &msg, &ctx).await?,
    }
    Ok(())
}

async fn text_message_handler(bot: Bot, msg: Message, ctx: Arc<BotContext>) -> ResponseResult<()> {
    let chat_id = msg.chat.id;
    let text = match msg.text() {
        Some(t) => t.trim(),
        None => {
            // Check if it's a document upload for storage browse
            if let Some(doc) = msg.document() {
                // State is keyed by chat_id: without this check any user in a
                // group chat could upload through an admin's browse session.
                let user_id = msg.from().map_or(0, |u| u.id.0 as i64);
                let authorized = {
                    let cfg = ctx.config.read().await;
                    is_admin(user_id, &cfg)
                };
                if !authorized {
                    return Ok(());
                }

                let state_key = (chat_id, user_id);
                let mut states = ctx.user_states.lock().await;
                if let Some(UserState::StorageBrowse {
                    op_state: StorageOpState::AwaitingUpload,
                    storage_id,
                    root_path,
                    current_path,
                    browse_msg_id,
                    prompt_msg_id,
                    ..
                }) = states.get_mut(&state_key)
                {
                    let path = current_path.clone();
                    let msg_id = *browse_msg_id;
                    let prompt_id = *prompt_msg_id;
                    let sid = storage_id.clone();
                    let root = root_path.clone();
                    let file_name = doc
                        .file_name
                        .clone()
                        .unwrap_or_else(|| "upload_file".to_string())
                        .replace(['/', '\\'], "_");

                    // Reset op state (preserve storage_id/root_path so "back to
                    // storage list" keeps working).
                    *states.get_mut(&state_key).unwrap() = UserState::StorageBrowse {
                        storage_id: sid,
                        root_path: root,
                        current_path: path.clone(),
                        browse_msg_id: msg_id,
                        files: vec![],
                        page: 1,
                        op_state: StorageOpState::None,
                        prompt_msg_id: None,
                        pending_delete_path: None,
                    };
                    drop(states);

                    if let Some(pid) = prompt_id {
                        let _ = bot.delete_message(chat_id, pid).await;
                    }
                    let _ = bot.delete_message(chat_id, msg.id).await;

                    let status_msg = bot
                        .send_message(
                            chat_id,
                            format!("⏳ 正在上传 `{}`", escape_code(&file_name)),
                        )
                        .parse_mode(teloxide::types::ParseMode::MarkdownV2)
                        .await?;

                    let file_id = doc.file.id.clone();
                    let bot_clone = bot.clone();
                    let ctx_clone = ctx.clone();
                    tokio::spawn(async move {
                        match bot_clone.get_file(file_id).send().await {
                            Ok(tg_file) => {
                                let mut bytes = Vec::new();
                                // Download file
                                match bot_clone.download_file(&tg_file.path, &mut bytes).await {
                                    Ok(_) => {
                                        match ctx_clone
                                            .openlist
                                            .fs_put_bytes(bytes, &path, &file_name)
                                            .await
                                        {
                                            Ok(_) => {
                                                let _ = bot_clone
                                                    .edit_message_text(
                                                        chat_id,
                                                        status_msg.id,
                                                        format!(
                                                            "✅ `{}` 上传成功",
                                                            escape_code(&file_name)
                                                        ),
                                                    )
                                                    .parse_mode(
                                                        teloxide::types::ParseMode::MarkdownV2,
                                                    )
                                                    .await;
                                                if let Some(b_id) = msg_id {
                                                    refresh_browse_message(
                                                        &bot_clone, &ctx_clone, state_key, b_id,
                                                        &path,
                                                    )
                                                    .await;
                                                }
                                            }
                                            Err(e) => {
                                                let _ = bot_clone
                                                    .edit_message_text(
                                                        chat_id,
                                                        status_msg.id,
                                                        format!("❌ 上传失败: {}", e),
                                                    )
                                                    .await;
                                            }
                                        }
                                    }
                                    Err(e) => {
                                        let _ = bot_clone
                                            .edit_message_text(
                                                chat_id,
                                                status_msg.id,
                                                format!("❌ 下载文件失败: {}", e),
                                            )
                                            .await;
                                    }
                                }
                            }
                            Err(e) => {
                                let _ = bot_clone
                                    .edit_message_text(
                                        chat_id,
                                        status_msg.id,
                                        format!("❌ 获取文件失败: {}", e),
                                    )
                                    .await;
                            }
                        }
                    });
                }
            }
            return Ok(());
        }
    };

    let sender_id = msg.from().map_or(0, |u| u.id.0 as i64);
    let state_key = (chat_id, sender_id);

    let mut states = ctx.user_states.lock().await;
    let state = states.get(&state_key).cloned();

    // A pending search prompt is keyed by chat_id but belongs to the user who
    // opened it. In a group, ignore text from anyone else so we neither run a
    // search on their behalf nor delete their (unrelated) message.
    if let Some(UserState::AwaitingSearchQuery { user_id, .. }) = &state {
        if *user_id != sender_id {
            return Ok(());
        }
    }

    // Interactive flows other than search are admin-only. State is keyed by
    // chat_id, so without this check any user in a group chat could drive an
    // admin's session (mkdir name, download URL, config value).
    if !matches!(&state, None | Some(UserState::AwaitingSearchQuery { .. })) {
        let cfg = ctx.config.read().await;
        if !is_admin(sender_id, &cfg) {
            return Ok(());
        }
    }

    match state {
        Some(UserState::AwaitingSearchQuery { prompt_msg_id, .. }) => {
            states.remove(&state_key);
            drop(states);

            let _ = bot.delete_message(chat_id, msg.id).await;

            crate::handlers::search::handle_s_with_edit(
                bot,
                msg.clone(),
                text.to_string(),
                ctx,
                prompt_msg_id,
            )
            .await?;
        }
        Some(UserState::StorageBrowse {
            op_state: StorageOpState::AwaitingMkdir,
            storage_id,
            root_path,
            current_path,
            browse_msg_id,
            prompt_msg_id,
            ..
        }) => {
            let path = current_path.clone();
            let msg_id = browse_msg_id;
            let prompt_id = prompt_msg_id;

            // Reset op state (preserve storage_id/root_path so "back to
            // storage list" keeps working).
            states.insert(
                state_key,
                UserState::StorageBrowse {
                    storage_id: storage_id.clone(),
                    root_path: root_path.clone(),
                    current_path: path.clone(),
                    browse_msg_id: msg_id,
                    files: vec![],
                    page: 1,
                    op_state: StorageOpState::None,
                    prompt_msg_id: None,
                    pending_delete_path: None,
                },
            );
            drop(states);

            if let Some(pid) = prompt_id {
                let _ = bot.delete_message(chat_id, pid).await;
            }
            let _ = bot.delete_message(chat_id, msg.id).await;

            if !is_valid_path_component(text) {
                bot.send_message(chat_id, "❌ 文件夹名称不能为空，也不能包含 /、\\、. 或 ..")
                    .await?;
                return Ok(());
            }

            let new_dir_path = format!("{}/{}", path.trim_end_matches('/'), text);
            match ctx.openlist.fs_mkdir(&new_dir_path).await {
                Ok(_) => {
                    if let Some(b_id) = msg_id {
                        refresh_browse_message(&bot, &ctx, state_key, b_id, &path).await;
                    }
                }
                Err(e) => {
                    bot.send_message(chat_id, format!("❌ 创建文件夹失败: {}", e))
                        .await?;
                }
            }
        }
        Some(UserState::OfflineDownload {
            step: OdStep::EnteringUrl,
            tool,
            path,
            message_id,
            ..
        }) => {
            let tool_val = tool.clone().unwrap_or_else(|| "qbittorrent".to_string());
            let path_val = path.clone().unwrap_or_else(|| "/".to_string());
            let url_val = text.to_string();

            states.insert(
                state_key,
                UserState::OfflineDownload {
                    step: OdStep::Confirming,
                    message_id,
                    tool: tool.clone(),
                    path: path.clone(),
                    urls: vec![url_val.clone()],
                },
            );
            drop(states);

            if let Some(mid) = message_id {
                let keyboard = InlineKeyboardMarkup::new(vec![
                    vec![InlineKeyboardButton::callback("✅ 确认下载", "od_confirm")],
                    vec![InlineKeyboardButton::callback("❌ 取消", "od_cancel")],
                ]);
                bot.edit_message_text(
                    chat_id,
                    mid,
                    format!(
                        "📥 确认下载\n\n工具: {}\n路径: {}\n链接: {}\n",
                        tool_val, path_val, url_val
                    ),
                )
                .reply_markup(keyboard)
                .await?;
            }
        }
        Some(UserState::ConfigEdit {
            step: CfStep::EditConfig,
            config_key: Some(key),
            config_name: Some(name),
            ..
        }) => {
            states.remove(&state_key);
            drop(states);

            // Edit config item. The write lock must be released before any
            // Telegram API await, otherwise every auth check (config read)
            // blocks until the send completes.
            let proxy_enabled = match key.as_str() {
                "proxy.enable" => match text.to_ascii_lowercase().as_str() {
                    "true" => Some(true),
                    "false" => Some(false),
                    _ => {
                        bot.send_message(chat_id, "❌ 更新失败: 代理启用值必须是 true 或 false")
                            .await?;
                        return Ok(());
                    }
                },
                _ => None,
            };
            let proxy_port = if key == "proxy.port" {
                match text.parse::<u16>() {
                    Ok(port) if port > 0 => Some(port),
                    Ok(_) | Err(_) => {
                        bot.send_message(chat_id, "❌ 更新失败: 端口必须是 1 到 65535 的数字")
                            .await?;
                        return Ok(());
                    }
                }
            } else {
                None
            };
            if !CONFIG_ITEMS.iter().any(|(item_key, _)| *item_key == key) {
                bot.send_message(chat_id, "❌ 更新失败: 未知配置项").await?;
                return Ok(());
            }

            let save_result = {
                let mut cfg = ctx.config.write().await;
                cfg.update_and_save(|candidate| match key.as_str() {
                    "openlist.download_path" => candidate.openlist.download_path = text.to_string(),
                    "openlist.download_tool" => candidate.openlist.download_tool = text.to_string(),
                    "proxy.enable" | "proxy.hostname" | "proxy.port" => {
                        let proxy =
                            candidate
                                .proxy
                                .get_or_insert_with(|| crate::config::ProxyConfig {
                                    enable: false,
                                    hostname: "127.0.0.1".to_string(),
                                    port: 1080,
                                    scheme: "http".to_string(),
                                });
                        match key.as_str() {
                            "proxy.enable" => proxy.enable = proxy_enabled.unwrap_or(false),
                            "proxy.hostname" => proxy.hostname = text.to_string(),
                            "proxy.port" => proxy.port = proxy_port.unwrap_or(1080),
                            _ => unreachable!(),
                        }
                    }
                    _ => unreachable!(),
                })
            };

            if let Err(e) = save_result {
                bot.send_message(chat_id, format!("❌ 保存失败: {}", e))
                    .await?;
            } else {
                bot.send_message(
                    chat_id,
                    format!(
                        "✅ 配置已更新！\n\n• {}: {}\n\n⚠️ 部分配置需要重启机器人后生效",
                        name, text
                    ),
                )
                .await?;
            }
        }
        _ => {}
    }

    Ok(())
}

async fn refresh_browse_message(
    bot: &Bot,
    ctx: &BotContext,
    state_key: (ChatId, i64),
    msg_id: MessageId,
    path: &str,
) {
    let chat_id = state_key.0;
    match ctx.openlist.fs_list(path).await {
        Ok(files) => {
            let mut states = ctx.user_states.lock().await;
            if let Some(UserState::StorageBrowse {
                files: cached_files,
                current_path,
                page,
                ..
            }) = states.get_mut(&state_key)
            {
                *cached_files = files.clone();
                *current_path = path.to_string();
                let current_page = *page;
                drop(states);

                let (text, keyboard) = build_file_list(ctx, &files, path, current_page).await;
                let _ = bot
                    .edit_message_text(chat_id, msg_id, text)
                    .reply_markup(keyboard)
                    .parse_mode(teloxide::types::ParseMode::MarkdownV2)
                    .await;
            }
        }
        Err(e) => {
            let _ = bot
                .send_message(chat_id, format!("刷新目录失败: {}", e))
                .await;
        }
    }
}

async fn callback_handler(
    bot: Bot,
    query: CallbackQuery,
    ctx: Arc<BotContext>,
) -> ResponseResult<()> {
    let msg = match query.message {
        Some(m) => m,
        None => return Ok(()),
    };
    let chat_id = msg.chat.id;
    let data = match query.data {
        Some(d) => d,
        None => return Ok(()),
    };

    // Authorization: every callback that browses storage, submits downloads,
    // deletes files or edits config is admin-only. Members (when a member list
    // is configured) may only page/filter search results, copy links, and open
    // the search prompt. State is keyed by chat_id, so without this check any
    // user in a shared group chat could drive another user's admin session.
    let clicker_id = query.from.id.0 as i64;
    let state_key = (chat_id, clicker_id);
    let member_only_ok = data.starts_with("search_")
        || data.starts_with("s_cp_")
        || data == "cfg_src_close"
        || data == "cfg_src_cancel_input"
        || data == "cfg_src_start_search";
    let authorized = {
        let cfg = ctx.config.read().await;
        is_admin(clicker_id, &cfg) || (member_only_ok && is_member(chat_id.0, clicker_id, &cfg))
    };
    if !authorized {
        let _ = bot
            .answer_callback_query(query.id)
            .text("⛔ 你没有权限执行此操作")
            .await;
        return Ok(());
    }

    let toast_text = if data.starts_with("cd_")
        || data.starts_with("storage_")
        || data == "back"
        || data.starts_with("cf_path_")
        || data.starts_with("cf_dir_")
        || data.starts_with("od_cd_")
        || data.starts_with("sb_cd_")
    {
        "⏳ 正在加载目录..."
    } else if data.starts_with("s_dl_") || data.starts_with("tb_add_") || data == "od_confirm" {
        "⏳ 正在提交下载任务..."
    } else if data == "del_confirm" {
        "⏳ 正在删除文件..."
    } else if data.starts_with("fl_") {
        "⏳ 正在刷新缓存..."
    } else if data.starts_with("ods_detail_") {
        "⏳ 正在获取任务详情..."
    } else if data.starts_with("od_tool_")
        || data.starts_with("cf_tool_")
        || data == "sb_tool_select"
        || data == "cf_edit_download"
    {
        "⏳ 正在获取工具列表..."
    } else if data.starts_with("cfg_src_toggle_") {
        "⏳ 正在更新搜索源..."
    } else if data == "cfg_src_start_search" {
        "⏳ 请在下方输入搜索内容..."
    } else if data.starts_with("s_cp_") {
        "📋 磁力链已发送，轻触代码块即可复制"
    } else {
        "⏳ 正在处理中..."
    };

    let _ = bot.answer_callback_query(query.id).text(toast_text).await;

    info!("CallbackQuery received: {}", data);

    // Search Config Menu callbacks
    if data.starts_with("cfg_src_") {
        if data == "cfg_src_close" {
            bot.delete_message(chat_id, msg.id).await?;
            return Ok(());
        }

        if data == "cfg_src_start_search" {
            let prompt = bot
                .edit_message_text(chat_id, msg.id, "🔎 请发送您要搜索的资源名称：")
                .reply_markup(InlineKeyboardMarkup::new(vec![vec![
                    InlineKeyboardButton::callback("❌ 取消", "cfg_src_cancel_input"),
                ]]))
                .await?;

            let mut states = ctx.user_states.lock().await;
            states.insert(
                state_key,
                UserState::AwaitingSearchQuery {
                    prompt_msg_id: Some(prompt.id),
                    user_id: clicker_id,
                },
            );
            return Ok(());
        }

        if data == "cfg_src_cancel_input" {
            let mut states = ctx.user_states.lock().await;
            states.remove(&state_key);
            drop(states);

            let (text, keyboard) = crate::handlers::search::build_search_config_menu(&ctx).await;
            bot.edit_message_text(chat_id, msg.id, text)
                .reply_markup(keyboard)
                .await?;
            return Ok(());
        }

        if data.starts_with("cfg_src_toggle_") {
            let key = data
                .strip_prefix("cfg_src_toggle_")
                .unwrap_or(&data)
                .to_string();
            let save_result = {
                let mut cfg = ctx.config.write().await;
                cfg.update_and_save(|candidate| {
                    let allowed = &mut candidate.search.allowed_sources;
                    if allowed.contains(&key) {
                        allowed.retain(|item| item != &key);
                    } else {
                        allowed.push(key.clone());
                    }
                })
            };
            if let Err(e) = save_result {
                bot.send_message(chat_id, format!("❌ 搜索源配置保存失败: {}", e))
                    .await?;
                return Ok(());
            }

            let (text, keyboard) = crate::handlers::search::build_search_config_menu(&ctx).await;
            bot.edit_message_text(chat_id, msg.id, text)
                .reply_markup(keyboard)
                .await?;
            return Ok(());
        }
    }

    // Copy magnet/ed2k link callback
    if data.starts_with("s_cp_") {
        let rest = data.strip_prefix("s_cp_").unwrap_or(&data).to_string();
        if let Some(pos) = rest.rfind('_') {
            let cmid_part = &rest[..pos];
            if let Ok(global_idx) = rest[pos + 1..].parse::<usize>() {
                let item_url = {
                    let pansou_results = ctx.pansou_results.lock().await;
                    pansou_results
                        .get(&format!("{}_{}", cmid_part, global_idx))
                        .map(|item| item.url.clone())
                };
                if let Some(url) = item_url {
                    let escaped_url = url.replace('\\', "\\\\").replace('`', "\\`");
                    let _ = bot
                        .send_message(chat_id, format!("`{}`", escaped_url))
                        .parse_mode(teloxide::types::ParseMode::MarkdownV2)
                        .await;
                }
            }
        }
        return Ok(());
    }

    // PanSou search pagination/filtering
    if data == "search_next"
        || data == "search_prev"
        || data == "search_filter_all"
        || data.starts_with("search_filter_")
    {
        let cmid = format!("{}|{}", chat_id, msg.id);
        let mut pansou_pages = ctx.pansou_pages.lock().await;
        if let Some(page) = pansou_pages.get_mut(&cmid) {
            if data == "search_next" {
                if page.index < page.page_count() - 1 {
                    page.index += 1;
                }
            } else if data == "search_prev" {
                if page.index > 0 {
                    page.index -= 1;
                }
            } else if data == "search_filter_all" {
                page.filter_type = None;
                page.index = 0;
            } else if data.starts_with("search_filter_") {
                let ptype = data
                    .strip_prefix("search_filter_")
                    .unwrap_or(&data)
                    .to_string();
                page.filter_type = Some(ptype);
                page.index = 0;
            }

            let text = page.get_text_with_info();
            let keyboard = page.btn();
            bot.edit_message_text(chat_id, msg.id, text)
                .reply_markup(keyboard)
                .parse_mode(teloxide::types::ParseMode::MarkdownV2)
                .disable_web_page_preview(true)
                .await?;
        } else {
            bot.send_message(chat_id, "搜索结果已过期，请重新搜索")
                .await?;
        }
        return Ok(());
    }

    // PanSou / Torrent download buttons callback
    if data.starts_with("s_dl_") {
        // Format: s_dl_{cmid}_{global_idx}
        let rest = data.strip_prefix("s_dl_").unwrap_or(&data).to_string();
        if let Some(pos) = rest.rfind('_') {
            let cmid_part = &rest[..pos];
            if let Ok(global_idx) = rest[pos + 1..].parse::<usize>() {
                let item = {
                    let pansou_results = ctx.pansou_results.lock().await;
                    pansou_results
                        .get(&format!("{}_{}", cmid_part, global_idx))
                        .cloned()
                };
                if let Some(item) = item {
                    let (dl_path, dl_tool) = {
                        let cfg = ctx.config.read().await;
                        (
                            cfg.openlist.download_path.clone(),
                            cfg.openlist.download_tool.clone(),
                        )
                    };

                    let file_name = if item.name.chars().count() > 30 {
                        item.name.chars().take(30).collect::<String>() + "..."
                    } else {
                        item.name.clone()
                    };

                    let new_state = UserState::TorrentDownloadSelect {
                        index: global_idx,
                        cmid: cmid_part.to_string(),
                        path: dl_path,
                        tool: dl_tool,
                        url: item.url.clone(),
                        file_name,
                        current_path: None,
                    };

                    let (text, keyboard) = build_download_confirm_message(&new_state).await;
                    {
                        let mut states = ctx.user_states.lock().await;
                        states.insert(state_key, new_state);
                    }
                    bot.edit_message_text(chat_id, msg.id, text)
                        .reply_markup(keyboard)
                        .await?;
                } else {
                    bot.send_message(chat_id, "链接已过期，请重新搜索").await?;
                }
            }
        }
        return Ok(());
    }

    // Torrent Download Config Select (sb_path_select, sb_tool_select, tb_add_)
    if data == "sb_path_select"
        || data == "sb_tool_select"
        || data.starts_with("tb_add_")
        || data.starts_with("sb_tool_")
        || data.starts_with("sb_cd_")
        || data == "sb_confirm_path"
        || data.starts_with("sb_cancel")
    {
        let state = {
            let states = ctx.user_states.lock().await;
            states.get(&state_key).cloned()
        };

        if let Some(UserState::TorrentDownloadSelect {
            index,
            cmid,
            path,
            tool,
            url,
            file_name,
            current_path,
        }) = state
        {
            if data == "sb_path_select" {
                // List storages
                match ctx.openlist.storage_list().await {
                    Ok(storages) => {
                        let mut buttons = Vec::new();
                        for st in storages {
                            if st.disabled {
                                continue;
                            }
                            let name = st
                                .remark
                                .as_deref()
                                .unwrap_or(st.mount_path.as_deref().unwrap_or("/"));
                            let mpath = st.mount_path.as_deref().unwrap_or("/");
                            let path_id = register_path(&ctx, mpath).await;
                            buttons.push(vec![InlineKeyboardButton::callback(
                                format!("📁 {}", name),
                                format!("sb_cd_{}", path_id),
                            )]);
                        }
                        buttons.push(vec![InlineKeyboardButton::callback(
                            "❌ 取消",
                            "sb_cancel_path",
                        )]);

                        {
                            let mut states = ctx.user_states.lock().await;
                            states.insert(
                                state_key,
                                UserState::TorrentDownloadSelect {
                                    index,
                                    cmid: cmid.clone(),
                                    path: path.clone(),
                                    tool: tool.clone(),
                                    url: url.clone(),
                                    file_name: file_name.clone(),
                                    current_path: Some("/".to_string()),
                                },
                            );
                        }

                        bot.edit_message_text(chat_id, msg.id, "📂 选择存储路径:")
                            .reply_markup(InlineKeyboardMarkup::new(buttons))
                            .await?;
                    }
                    Err(e) => {
                        bot.send_message(chat_id, format!("获取存储列表失败: {}", e))
                            .await?;
                    }
                }
            } else if data.starts_with("sb_cd_") {
                let path_id = data.strip_prefix("sb_cd_").unwrap_or(&data).to_string();
                if let Some(cd_path) = get_path(&ctx, &path_id).await {
                    {
                        let mut states = ctx.user_states.lock().await;
                        states.insert(
                            state_key,
                            UserState::TorrentDownloadSelect {
                                index,
                                cmid: cmid.clone(),
                                path: path.clone(),
                                tool: tool.clone(),
                                url: url.clone(),
                                file_name: file_name.clone(),
                                current_path: Some(cd_path.clone()),
                            },
                        );
                    }

                    // Browser list
                    match ctx.openlist.fs_list(&cd_path).await {
                        Ok(files) => {
                            let dirs: Vec<_> = files.iter().filter(|f| f.is_dir).collect();
                            let mut buttons = Vec::new();
                            for f in dirs.iter().take(10) {
                                let sub = format!(
                                    "{}/{}",
                                    cd_path.trim_end_matches('/'),
                                    f.name.trim_start_matches('/')
                                );
                                let sub_id = register_path(&ctx, &sub).await;
                                buttons.push(vec![InlineKeyboardButton::callback(
                                    format!("📁 {}", f.name),
                                    format!("sb_cd_{}", sub_id),
                                )]);
                            }
                            buttons.push(vec![InlineKeyboardButton::callback(
                                "✅ 确认此路径",
                                "sb_confirm_path",
                            )]);

                            if cd_path != "/" {
                                let parent = parent_path(&cd_path);
                                let parent_id = register_path(&ctx, &parent).await;
                                buttons.push(vec![InlineKeyboardButton::callback(
                                    "⬅️ 返回上一级",
                                    format!("sb_cd_{}", parent_id),
                                )]);
                            }

                            let mut text = format!(
                                "📂 选择下载路径: `{}`\n\n点击目录进入，点击确认此路径按钮完成选择",
                                escape_code(&cd_path)
                            );
                            if dirs.len() > 10 {
                                text.push_str(&format!(
                                    "\n\n⚠️ 目录过多，仅显示前 10 个（共 {} 个）",
                                    dirs.len()
                                ));
                            }
                            bot.edit_message_text(chat_id, msg.id, text)
                                .reply_markup(InlineKeyboardMarkup::new(buttons))
                                .parse_mode(teloxide::types::ParseMode::MarkdownV2)
                                .await?;
                        }
                        Err(e) => {
                            bot.send_message(chat_id, format!("获取目录失败: {}", e))
                                .await?;
                        }
                    }
                }
            } else if data == "sb_confirm_path" {
                if let Some(cd_path) = current_path {
                    let confirm_state = UserState::TorrentDownloadSelect {
                        index,
                        cmid: cmid.clone(),
                        path: cd_path.clone(),
                        tool: tool.clone(),
                        url: url.clone(),
                        file_name: file_name.clone(),
                        current_path: None,
                    };
                    let (text, keyboard) = build_download_confirm_message(&confirm_state).await;
                    {
                        let mut states = ctx.user_states.lock().await;
                        states.insert(state_key, confirm_state);
                    }
                    bot.edit_message_text(chat_id, msg.id, text)
                        .reply_markup(keyboard)
                        .await?;
                }
            } else if data.starts_with("sb_cancel") {
                let cancel_state = UserState::TorrentDownloadSelect {
                    index,
                    cmid: cmid.clone(),
                    path: path.clone(),
                    tool: tool.clone(),
                    url: url.clone(),
                    file_name: file_name.clone(),
                    current_path: None,
                };
                let (text, keyboard) = build_download_confirm_message(&cancel_state).await;
                {
                    let mut states = ctx.user_states.lock().await;
                    states.insert(state_key, cancel_state);
                }
                bot.edit_message_text(chat_id, msg.id, text)
                    .reply_markup(keyboard)
                    .await?;
            } else if data == "sb_tool_select" {
                match ctx.openlist.get_offline_download_tools().await {
                    Ok(tools) => {
                        let mut buttons = Vec::new();
                        for t in tools {
                            let tool_id = register_path(&ctx, &t).await;
                            buttons.push(vec![InlineKeyboardButton::callback(
                                t.clone(),
                                format!("sb_tool_{}", tool_id),
                            )]);
                        }
                        buttons.push(vec![InlineKeyboardButton::callback(
                            "❌ 取消",
                            "sb_cancel_tool",
                        )]);
                        bot.edit_message_text(chat_id, msg.id, "🔧 选择下载工具:")
                            .reply_markup(InlineKeyboardMarkup::new(buttons))
                            .await?;
                    }
                    Err(e) => {
                        bot.send_message(chat_id, format!("获取工具失败: {}", e))
                            .await?;
                    }
                }
            } else if data.starts_with("sb_tool_") {
                let tool_id = data.strip_prefix("sb_tool_").unwrap_or(&data);
                let new_tool = get_path(&ctx, tool_id).await.unwrap_or_default();
                let confirm_state = UserState::TorrentDownloadSelect {
                    index,
                    cmid: cmid.clone(),
                    path: path.clone(),
                    tool: new_tool,
                    url: url.clone(),
                    file_name: file_name.clone(),
                    current_path: None,
                };
                let (text, keyboard) = build_download_confirm_message(&confirm_state).await;
                {
                    let mut states = ctx.user_states.lock().await;
                    states.insert(state_key, confirm_state);
                }
                bot.edit_message_text(chat_id, msg.id, text)
                    .reply_markup(keyboard)
                    .await?;
            } else if data.starts_with("tb_add_") {
                bot.send_message(chat_id, "🔄 准备下载...").await?;

                let urls = vec![url.clone()];
                let tool_val = tool.clone();
                let path_val = path.clone();
                let bot_clone = bot.clone();
                let ctx_clone = ctx.clone();

                {
                    let mut states = ctx.user_states.lock().await;
                    states.remove(&state_key);
                }

                tokio::spawn(async move {
                    match ctx_clone
                        .openlist
                        .add_offline_download(urls.clone(), &tool_val, &path_val)
                        .await
                    {
                        Ok(_) => {
                            let _ = bot_clone
                                .edit_message_text(
                                    chat_id,
                                    msg.id,
                                    format!(
                                        "✅ 下载任务已创建!\n\n工具: {}\n路径: {}\n链接: {}",
                                        tool_val, path_val, urls[0]
                                    ),
                                )
                                .await;
                        }
                        Err(e) => {
                            let _ = bot_clone
                                .edit_message_text(chat_id, msg.id, format!("❌ 创建失败: {}", e))
                                .await;
                        }
                    }
                });
                return Ok(());
            }
        }
    }

    // Storage Browse Interactive callback
    if data.starts_with("storage_")
        || data.starts_with("file_")
        || data.starts_with("cd_")
        || data == "browse_prev"
        || data == "browse_next"
        || data == "back"
        || data == "st_cancel"
    {
        if data == "st_cancel" {
            {
                let mut states = ctx.user_states.lock().await;
                states.remove(&state_key);
            }
            bot.edit_message_text(chat_id, msg.id, "❌ 已取消").await?;
            return Ok(());
        }

        if let Some(rest) = data.strip_prefix("storage_") {
            let parts: Vec<&str> = rest.splitn(2, ':').collect();
            let storage_id = parts[0].to_string();
            let mount_path = match parts.get(1) {
                Some(key) => get_path(&ctx, key).await.unwrap_or_else(|| "/".to_string()),
                None => "/".to_string(),
            };

            match ctx.openlist.fs_list(&mount_path).await {
                Ok(files) => {
                    let mut states = ctx.user_states.lock().await;
                    states.insert(
                        state_key,
                        UserState::StorageBrowse {
                            storage_id: storage_id.clone(),
                            root_path: mount_path.clone(),
                            current_path: mount_path.clone(),
                            browse_msg_id: Some(msg.id),
                            files: files.clone(),
                            page: 1,
                            op_state: StorageOpState::None,
                            prompt_msg_id: None,
                            pending_delete_path: None,
                        },
                    );
                    drop(states);

                    let (text, keyboard) = build_file_list(&ctx, &files, &mount_path, 1).await;
                    bot.edit_message_text(chat_id, msg.id, text)
                        .reply_markup(keyboard)
                        .parse_mode(teloxide::types::ParseMode::MarkdownV2)
                        .await?;
                }
                Err(e) => {
                    bot.send_message(chat_id, format!("获取目录失败: {}", e))
                        .await?;
                }
            }
            return Ok(());
        }

        let state = {
            let states = ctx.user_states.lock().await;
            states.get(&state_key).cloned()
        };

        if let Some(UserState::StorageBrowse {
            storage_id,
            root_path,
            current_path,
            files,
            page,
            op_state,
            prompt_msg_id,
            pending_delete_path,
            ..
        }) = state
        {
            if data.starts_with("file_") {
                let path_id = data.strip_prefix("file_").unwrap_or(&data).to_string();
                if let Some(file_path) = get_path(&ctx, &path_id).await {
                    if !path_is_direct_child(&file_path, &current_path, &root_path) {
                        bot.send_message(chat_id, "按钮已过期，请使用当前目录中的按钮")
                            .await?;
                        return Ok(());
                    }
                    let bot_clone = bot.clone();
                    let ctx_clone = ctx.clone();
                    tokio::spawn(async move {
                        match ctx_clone.openlist.fs_get(&file_path).await {
                            Ok(info) => {
                                let filename = file_path.split('/').next_back().unwrap_or("");
                                let web_url =
                                    ctx_clone.config.read().await.openlist.openlist_host.clone();
                                let full_web_url =
                                    format!("{}/#{}", web_url.trim_end_matches('/'), file_path);

                                let text_resp = if let Some(raw) = info.raw_url {
                                    format!(
                                        "文件: `{}`\n\n直链: `{}`\n\n打开链接: `{}`",
                                        escape_code(filename),
                                        escape_code(&raw),
                                        escape_code(&full_web_url)
                                    )
                                } else {
                                    format!(
                                        "文件: `{}`\n\n打开链接: `{}`",
                                        escape_code(filename),
                                        escape_code(&full_web_url)
                                    )
                                };
                                let _ = bot_clone
                                    .send_message(chat_id, text_resp)
                                    .disable_web_page_preview(true)
                                    .parse_mode(teloxide::types::ParseMode::MarkdownV2)
                                    .await;
                            }
                            Err(e) => {
                                let _ = bot_clone
                                    .send_message(chat_id, format!("获取链接失败: {}", e))
                                    .await;
                            }
                        }
                    });
                }
            } else if data.starts_with("cd_") {
                let path_id = data.strip_prefix("cd_").unwrap_or(&data).to_string();
                info!("cd_ callback received, path_id: {}", path_id);
                if let Some(target_path) = get_path(&ctx, &path_id).await {
                    if !path_is_direct_child(&target_path, &current_path, &root_path) {
                        bot.send_message(chat_id, "按钮已过期，请使用当前目录中的按钮")
                            .await?;
                        return Ok(());
                    }
                    info!("cd_ target_path: {}", target_path);
                    match ctx.openlist.fs_list(&target_path).await {
                        Ok(new_files) => {
                            {
                                let mut states = ctx.user_states.lock().await;
                                states.insert(
                                    state_key,
                                    UserState::StorageBrowse {
                                        storage_id: storage_id.clone(),
                                        root_path: root_path.clone(),
                                        current_path: target_path.clone(),
                                        browse_msg_id: Some(msg.id),
                                        files: new_files.clone(),
                                        page: 1,
                                        op_state: op_state.clone(),
                                        prompt_msg_id,
                                        pending_delete_path: pending_delete_path.clone(),
                                    },
                                );
                            }

                            let (text, keyboard) =
                                build_file_list(&ctx, &new_files, &target_path, 1).await;
                            bot.edit_message_text(chat_id, msg.id, text)
                                .reply_markup(keyboard)
                                .parse_mode(teloxide::types::ParseMode::MarkdownV2)
                                .await?;
                        }
                        Err(e) => {
                            bot.send_message(chat_id, format!("进入目录失败: {}", e))
                                .await?;
                        }
                    }
                } else {
                    info!("cd_ path_id {} not found in registry", path_id);
                }
            } else if data == "browse_prev" || data == "browse_next" {
                let total_pages = if files.is_empty() {
                    1
                } else {
                    files
                        .len()
                        .div_ceil(crate::handlers::storage_browse::PER_PAGE)
                };
                let new_page = if data == "browse_prev" && page > 1 {
                    page - 1
                } else if data == "browse_next" && page < total_pages {
                    page + 1
                } else {
                    page
                };

                {
                    let mut states = ctx.user_states.lock().await;
                    states.insert(
                        state_key,
                        UserState::StorageBrowse {
                            storage_id: storage_id.clone(),
                            root_path: root_path.clone(),
                            current_path: current_path.clone(),
                            browse_msg_id: Some(msg.id),
                            files: files.clone(),
                            page: new_page,
                            op_state: op_state.clone(),
                            prompt_msg_id,
                            pending_delete_path: pending_delete_path.clone(),
                        },
                    );
                }

                let (text, keyboard) = build_file_list(&ctx, &files, &current_path, new_page).await;
                bot.edit_message_text(chat_id, msg.id, text)
                    .reply_markup(keyboard)
                    .parse_mode(teloxide::types::ParseMode::MarkdownV2)
                    .await?;
            } else if data == "back" {
                if current_path == root_path {
                    // Go back to storage list - edit current message inline
                    {
                        let mut states = ctx.user_states.lock().await;
                        states.remove(&state_key);
                    }

                    match ctx.openlist.storage_list().await {
                        Ok(storages) => {
                            if storages.is_empty() {
                                bot.edit_message_text(chat_id, msg.id, "暂无存储").await?;
                                return Ok(());
                            }
                            let mut buttons = Vec::new();
                            for st in storages {
                                let remark = st.remark.as_deref().unwrap_or("");
                                let mount = st.mount_path.as_deref().unwrap_or("/");
                                let name = if !remark.is_empty() { remark } else { mount };
                                let status = if st.disabled { "❌" } else { "✅" };
                                let mount_id = register_path(&ctx, mount).await;
                                buttons.push(vec![InlineKeyboardButton::callback(
                                    format!("{}{}", status, name),
                                    format!("storage_{}:{}", st.id, mount_id),
                                )]);
                            }
                            buttons
                                .push(vec![InlineKeyboardButton::callback("❌ 取消", "st_cancel")]);
                            bot.edit_message_text(chat_id, msg.id, "选择存储:")
                                .reply_markup(InlineKeyboardMarkup::new(buttons))
                                .await?;
                        }
                        Err(e) => {
                            bot.edit_message_text(
                                chat_id,
                                msg.id,
                                format!("获取存储列表失败: {}", e),
                            )
                            .await?;
                        }
                    }
                    return Ok(());
                }

                let parent = parent_path(&current_path);
                let actual_parent = if path_is_within(&parent, &root_path) {
                    parent
                } else {
                    root_path.clone()
                };

                match ctx.openlist.fs_list(&actual_parent).await {
                    Ok(new_files) => {
                        {
                            let mut states = ctx.user_states.lock().await;
                            states.insert(
                                state_key,
                                UserState::StorageBrowse {
                                    storage_id: storage_id.clone(),
                                    root_path: root_path.clone(),
                                    current_path: actual_parent.clone(),
                                    browse_msg_id: Some(msg.id),
                                    files: new_files.clone(),
                                    page: 1,
                                    op_state: op_state.clone(),
                                    prompt_msg_id,
                                    pending_delete_path: pending_delete_path.clone(),
                                },
                            );
                        }

                        let (text, keyboard) =
                            build_file_list(&ctx, &new_files, &actual_parent, 1).await;
                        bot.edit_message_text(chat_id, msg.id, text)
                            .reply_markup(keyboard)
                            .parse_mode(teloxide::types::ParseMode::MarkdownV2)
                            .await?;
                    }
                    Err(e) => {
                        bot.send_message(chat_id, format!("进入上级目录失败: {}", e))
                            .await?;
                    }
                }
            }
        } else {
            bot.send_message(chat_id, "会话已过期，请重新发送 /browse")
                .await?;
            return Ok(());
        }
    }

    // Storage Browse deletion callbacks (del_, del_confirm, del_cancel_msg)
    if data.starts_with("del_") || data == "del_confirm" || data == "del_cancel_msg" {
        let state = {
            let states = ctx.user_states.lock().await;
            states.get(&state_key).cloned()
        };

        if let Some(UserState::StorageBrowse {
            storage_id,
            root_path,
            current_path,
            browse_msg_id,
            files,
            page,
            op_state,
            prompt_msg_id,
            pending_delete_path,
        }) = state
        {
            // NOTE: order matters — "del_confirm"/"del_cancel_msg" also start with
            // "del_", so the exact matches must be handled first (or excluded here),
            // otherwise clicking 确认/取消 would fall into the generic branch and
            // silently do nothing.
            if data.starts_with("del_") && data != "del_confirm" && data != "del_cancel_msg" {
                let path_id = data.strip_prefix("del_").unwrap_or(&data).to_string();
                if let Some(del_path) = get_path(&ctx, &path_id).await {
                    if !path_is_direct_child(&del_path, &current_path, &root_path) {
                        bot.send_message(chat_id, "按钮已过期，请使用当前目录中的按钮")
                            .await?;
                        return Ok(());
                    }
                    let item_name = del_path.split('/').next_back().unwrap_or("");
                    let keyboard = InlineKeyboardMarkup::new(vec![vec![
                        InlineKeyboardButton::callback("✅ 确认删除", "del_confirm"),
                        InlineKeyboardButton::callback("❌ 取消", "del_cancel_msg"),
                    ]]);

                    let confirmation = bot
                        .send_message(
                            chat_id,
                            format!(
                                "确认删除 `{}`？\n\n此操作不可撤销！",
                                escape_code(item_name)
                            ),
                        )
                        .reply_markup(keyboard)
                        .parse_mode(teloxide::types::ParseMode::MarkdownV2)
                        .await?;

                    let mut states = ctx.user_states.lock().await;
                    states.insert(
                        state_key,
                        UserState::StorageBrowse {
                            storage_id: storage_id.clone(),
                            root_path: root_path.clone(),
                            current_path: current_path.clone(),
                            browse_msg_id,
                            files: files.clone(),
                            page,
                            op_state: op_state.clone(),
                            prompt_msg_id,
                            pending_delete_path: Some((del_path, confirmation.id)),
                        },
                    );
                }
            } else if data == "del_confirm" {
                if let Some((del_path, confirmation_id)) = pending_delete_path {
                    if msg.id != confirmation_id
                        || !path_is_direct_child(&del_path, &current_path, &root_path)
                    {
                        bot.edit_message_text(chat_id, msg.id, "❌ 此删除确认已过期")
                            .await?;
                        return Ok(());
                    }
                    let dir_path = parent_path(&del_path);
                    let item_name = del_path.split('/').next_back().unwrap_or("").to_string();

                    let bot_clone = bot.clone();
                    let ctx_clone = ctx.clone();
                    let b_id = browse_msg_id;
                    let cur_path = current_path.clone();

                    {
                        let mut states = ctx.user_states.lock().await;
                        states.insert(
                            state_key,
                            UserState::StorageBrowse {
                                storage_id: storage_id.clone(),
                                root_path: root_path.clone(),
                                current_path: current_path.clone(),
                                browse_msg_id,
                                files: files.clone(),
                                page,
                                op_state: op_state.clone(),
                                prompt_msg_id,
                                pending_delete_path: None,
                            },
                        );
                    }

                    bot.edit_message_text(chat_id, msg.id, "🔄 正在删除...")
                        .await?;

                    tokio::spawn(async move {
                        match ctx_clone
                            .openlist
                            .fs_remove(&dir_path, vec![item_name.clone()])
                            .await
                        {
                            Ok(_) => {
                                let _ = bot_clone
                                    .edit_message_text(
                                        chat_id,
                                        msg.id,
                                        format!("✅ 已删除 `{}`", escape_code(&item_name)),
                                    )
                                    .parse_mode(teloxide::types::ParseMode::MarkdownV2)
                                    .await;
                                if let Some(b_msg_id) = b_id {
                                    refresh_browse_message(
                                        &bot_clone, &ctx_clone, state_key, b_msg_id, &cur_path,
                                    )
                                    .await;
                                }
                            }
                            Err(e) => {
                                let _ = bot_clone
                                    .edit_message_text(
                                        chat_id,
                                        msg.id,
                                        format!("❌ 删除失败: {}", e),
                                    )
                                    .await;
                            }
                        }
                    });
                }
            } else if data == "del_cancel_msg" {
                if pending_delete_path
                    .as_ref()
                    .is_some_and(|(_, confirmation_id)| *confirmation_id != msg.id)
                {
                    bot.edit_message_text(chat_id, msg.id, "❌ 此删除确认已过期")
                        .await?;
                    return Ok(());
                }
                {
                    let mut states = ctx.user_states.lock().await;
                    states.insert(
                        state_key,
                        UserState::StorageBrowse {
                            storage_id: storage_id.clone(),
                            root_path: root_path.clone(),
                            current_path: current_path.clone(),
                            browse_msg_id,
                            files: files.clone(),
                            page,
                            op_state: op_state.clone(),
                            prompt_msg_id,
                            pending_delete_path: None,
                        },
                    );
                }
                bot.edit_message_text(chat_id, msg.id, "❌ 已取消").await?;
            }
        }
    }

    // Storage Browse Operation callbacks (st_mkdir, st_upload, st_op_cancel)
    if data == "st_mkdir" || data == "st_upload" || data == "st_op_cancel" {
        let state = {
            let states = ctx.user_states.lock().await;
            states.get(&state_key).cloned()
        };

        if let Some(UserState::StorageBrowse {
            storage_id,
            root_path,
            current_path,
            browse_msg_id,
            files,
            page,
            op_state: _,
            prompt_msg_id,
            pending_delete_path,
        }) = state
        {
            if data == "st_op_cancel" {
                {
                    let mut states = ctx.user_states.lock().await;
                    states.insert(
                        state_key,
                        UserState::StorageBrowse {
                            storage_id: storage_id.clone(),
                            root_path: root_path.clone(),
                            current_path: current_path.clone(),
                            browse_msg_id,
                            files: files.clone(),
                            page,
                            op_state: StorageOpState::None,
                            prompt_msg_id: None,
                            pending_delete_path: pending_delete_path.clone(),
                        },
                    );
                }
                if let Some(pid) = prompt_msg_id {
                    let _ = bot.delete_message(chat_id, pid).await;
                }
                bot.edit_message_text(chat_id, msg.id, "❌ 已取消").await?;
            } else if data == "st_mkdir" {
                let keyboard =
                    InlineKeyboardMarkup::new(vec![vec![InlineKeyboardButton::callback(
                        "❌ 取消",
                        "st_op_cancel",
                    )]]);
                let prompt = bot
                    .send_message(chat_id, "请输入新文件夹名称：")
                    .reply_markup(keyboard)
                    .await?;

                let mut states = ctx.user_states.lock().await;
                states.insert(
                    state_key,
                    UserState::StorageBrowse {
                        storage_id: storage_id.clone(),
                        root_path: root_path.clone(),
                        current_path: current_path.clone(),
                        browse_msg_id,
                        files: files.clone(),
                        page,
                        op_state: StorageOpState::AwaitingMkdir,
                        prompt_msg_id: Some(prompt.id),
                        pending_delete_path: pending_delete_path.clone(),
                    },
                );
            } else if data == "st_upload" {
                let keyboard =
                    InlineKeyboardMarkup::new(vec![vec![InlineKeyboardButton::callback(
                        "❌ 取消",
                        "st_op_cancel",
                    )]]);
                let prompt = bot
                    .send_message(chat_id, "请发送要上传的文件：")
                    .reply_markup(keyboard)
                    .await?;

                let mut states = ctx.user_states.lock().await;
                states.insert(
                    state_key,
                    UserState::StorageBrowse {
                        storage_id: storage_id.clone(),
                        root_path: root_path.clone(),
                        current_path: current_path.clone(),
                        browse_msg_id,
                        files: files.clone(),
                        page,
                        op_state: StorageOpState::AwaitingUpload,
                        prompt_msg_id: Some(prompt.id),
                        pending_delete_path: pending_delete_path.clone(),
                    },
                );
            }
        }
    }

    // Offline Download Interactive callbacks (od_tool_, od_confirm_path, od_confirm, od_cancel, od_cd_)
    if data.starts_with("od_tool_")
        || data == "od_confirm_path"
        || data == "od_confirm"
        || data == "od_cancel"
        || data.starts_with("od_cd_")
    {
        let state = {
            let states = ctx.user_states.lock().await;
            states.get(&state_key).cloned()
        };

        if let Some(UserState::OfflineDownload {
            step: _,
            message_id,
            tool,
            path,
            urls,
        }) = state
        {
            if data.starts_with("od_tool_") {
                let tool_id = data.strip_prefix("od_tool_").unwrap_or(&data);
                let selected_tool = get_path(&ctx, tool_id).await.unwrap_or_default();
                // Fetch storage list
                match ctx.openlist.storage_list().await {
                    Ok(storages) => {
                        let mut buttons = Vec::new();
                        for st in storages {
                            if st.disabled {
                                continue;
                            }
                            let name = st
                                .remark
                                .as_deref()
                                .unwrap_or(st.mount_path.as_deref().unwrap_or("/"));
                            let mpath = st.mount_path.as_deref().unwrap_or("/");
                            let path_id = register_path(&ctx, mpath).await;
                            buttons.push(vec![InlineKeyboardButton::callback(
                                format!("📁 {}", name),
                                format!("od_cd_{}", path_id),
                            )]);
                        }
                        buttons.push(vec![InlineKeyboardButton::callback(
                            "✅ 确认选择路径",
                            "od_confirm_path",
                        )]);

                        {
                            let mut states = ctx.user_states.lock().await;
                            states.insert(
                                state_key,
                                UserState::OfflineDownload {
                                    step: OdStep::BrowsingPath,
                                    message_id,
                                    tool: Some(selected_tool.clone()),
                                    path: Some("/".to_string()),
                                    urls: vec![],
                                },
                            );
                        }

                        bot.edit_message_text(
                            chat_id,
                            msg.id,
                            format!(
                                "✅ 已选择工具: {}\n\n请选择存储（点击文件夹进入）:",
                                selected_tool
                            ),
                        )
                        .reply_markup(InlineKeyboardMarkup::new(buttons))
                        .await?;
                    }
                    Err(e) => {
                        bot.send_message(chat_id, format!("获取存储列表失败: {}", e))
                            .await?;
                    }
                }
            } else if data.starts_with("od_cd_") {
                let path_id = data.strip_prefix("od_cd_").unwrap_or(&data).to_string();
                if let Some(cd_path) = get_path(&ctx, &path_id).await {
                    {
                        let mut states = ctx.user_states.lock().await;
                        states.insert(
                            state_key,
                            UserState::OfflineDownload {
                                step: OdStep::BrowsingPath,
                                message_id,
                                tool: tool.clone(),
                                path: Some(cd_path.clone()),
                                urls: vec![],
                            },
                        );
                    }

                    // Browser list
                    match ctx.openlist.fs_list(&cd_path).await {
                        Ok(files) => {
                            let dirs: Vec<_> = files.iter().filter(|f| f.is_dir).collect();
                            let mut buttons = Vec::new();
                            for f in dirs.iter().take(10) {
                                let sub = format!(
                                    "{}/{}",
                                    cd_path.trim_end_matches('/'),
                                    f.name.trim_start_matches('/')
                                );
                                let sub_id = register_path(&ctx, &sub).await;
                                buttons.push(vec![InlineKeyboardButton::callback(
                                    format!("📁 {}", f.name),
                                    format!("od_cd_{}", sub_id),
                                )]);
                            }
                            buttons.push(vec![InlineKeyboardButton::callback(
                                "✅ 确认此路径",
                                "od_confirm_path",
                            )]);

                            if cd_path != "/" {
                                let parent = parent_path(&cd_path);
                                let parent_id = register_path(&ctx, &parent).await;
                                buttons.push(vec![InlineKeyboardButton::callback(
                                    "⬅️ 返回上级",
                                    format!("od_cd_{}", parent_id),
                                )]);
                            }

                            let mut text = format!(
                                "📁 选择存储路径: `{}`\n\n点击目录进入，点击确认此路径按钮完成选择",
                                escape_code(&cd_path)
                            );
                            if dirs.len() > 10 {
                                text.push_str(&format!(
                                    "\n\n⚠️ 目录过多，仅显示前 10 个（共 {} 个）",
                                    dirs.len()
                                ));
                            }
                            bot.edit_message_text(chat_id, msg.id, text)
                                .reply_markup(InlineKeyboardMarkup::new(buttons))
                                .parse_mode(teloxide::types::ParseMode::MarkdownV2)
                                .await?;
                        }
                        Err(e) => {
                            bot.send_message(chat_id, format!("获取目录失败: {}", e))
                                .await?;
                        }
                    }
                }
            } else if data == "od_confirm_path" {
                let cur_path = path.clone().unwrap_or_else(|| "/".to_string());
                {
                    let mut states = ctx.user_states.lock().await;
                    states.insert(
                        state_key,
                        UserState::OfflineDownload {
                            step: OdStep::EnteringUrl,
                            message_id,
                            tool: tool.clone(),
                            path: Some(cur_path.clone()),
                            urls: vec![],
                        },
                    );
                }
                bot.edit_message_text(
                    chat_id,
                    msg.id,
                    format!(
                        "已选择路径: {}\n\n请输入下载链接 (支持 HTTP/magnet/bt):",
                        cur_path
                    ),
                )
                .await?;
            } else if data == "od_confirm" {
                let tool_val = tool.clone().unwrap_or_else(|| "qbittorrent".to_string());
                let path_val = path.clone().unwrap_or_else(|| "/".to_string());
                let url_val = urls.first().cloned().unwrap_or_default();

                {
                    let mut states = ctx.user_states.lock().await;
                    states.remove(&state_key);
                }

                bot.edit_message_text(chat_id, msg.id, "🔄 正在创建任务...")
                    .await?;

                let bot_clone = bot.clone();
                let ctx_clone = ctx.clone();
                tokio::spawn(async move {
                    match ctx_clone
                        .openlist
                        .add_offline_download(vec![url_val.clone()], &tool_val, &path_val)
                        .await
                    {
                        Ok(_) => {
                            let _ = bot_clone
                                .edit_message_text(
                                    chat_id,
                                    msg.id,
                                    format!(
                                        "✅ 下载任务已创建!\n\n工具: {}\n路径: {}\n链接: {}",
                                        tool_val, path_val, url_val
                                    ),
                                )
                                .await;
                        }
                        Err(e) => {
                            let _ = bot_clone
                                .edit_message_text(chat_id, msg.id, format!("❌ 创建失败: {}", e))
                                .await;
                        }
                    }
                });
            } else if data == "od_cancel" {
                {
                    let mut states = ctx.user_states.lock().await;
                    states.remove(&state_key);
                }
                bot.edit_message_text(chat_id, msg.id, "❌ 已取消").await?;
            }
        }
        return Ok(());
    }

    if data == "od_new" {
        let _ = bot.delete_message(chat_id, msg.id).await;
        start_od_download_flow(bot.clone(), msg.clone(), ctx.clone(), Some(clicker_id)).await?;
        return Ok(());
    }

    if data == "od_status" {
        let _ = bot.delete_message(chat_id, msg.id).await;
        handle_ods(bot.clone(), msg.clone(), ctx.clone(), 1, Some(clicker_id)).await?;
        return Ok(());
    }

    // Offline Download Status callbacks (ods_page_, ods_detail_, ods_close)
    if data.starts_with("ods_page_") || data.starts_with("ods_detail_") || data == "ods_close" {
        if data == "ods_close" {
            bot.delete_message(chat_id, msg.id).await?;
            return Ok(());
        }

        if data.starts_with("ods_page_") {
            if let Ok(p) = data
                .strip_prefix("ods_page_")
                .unwrap_or(&data)
                .parse::<usize>()
            {
                bot.delete_message(chat_id, msg.id).await?;
                handle_ods(bot.clone(), msg.clone(), ctx.clone(), p, Some(clicker_id)).await?;
            }
            return Ok(());
        }

        if data.starts_with("ods_detail_") {
            let task_id = data
                .strip_prefix("ods_detail_")
                .unwrap_or(&data)
                .to_string();

            // Query details
            bot.edit_message_text(chat_id, msg.id, "🔄 正在读取详情...")
                .await?;

            // Search task
            let (undone, done) = match tokio::try_join!(
                ctx.openlist.get_offline_download_undone_task(),
                ctx.openlist.get_offline_download_done_task()
            ) {
                Ok(tasks) => tasks,
                Err(e) => {
                    bot.edit_message_text(chat_id, msg.id, format!("获取任务详情失败: {}", e))
                        .await?;
                    return Ok(());
                }
            };

            let mut found_task = None;
            let mut is_undone = true;
            for t in undone {
                if t.id == task_id {
                    found_task = Some(t);
                    is_undone = true;
                    break;
                }
            }
            if found_task.is_none() {
                for t in done {
                    if t.id == task_id {
                        found_task = Some(t);
                        is_undone = false;
                        break;
                    }
                }
            }

            if let Some(task) = found_task {
                let (filename, target_path) = extract_file_info(&task.name);

                if is_undone {
                    let text_resp = format!(
                        "⏳ 任务详情\n\n文件名: {}\n进度: {:.0}%\n状态: {}\n路径: {}\n",
                        filename,
                        task.progress,
                        task.status.unwrap_or_default(),
                        target_path
                    );
                    let keyboard =
                        InlineKeyboardMarkup::new(vec![vec![InlineKeyboardButton::callback(
                            "⬅️ 返回列表",
                            "ods_page_1",
                        )]]);
                    bot.edit_message_text(chat_id, msg.id, text_resp)
                        .reply_markup(keyboard)
                        .await?;
                } else {
                    let size = format_size(task.total_bytes);
                    let bot_clone = bot.clone();
                    let ctx_clone = ctx.clone();

                    tokio::spawn(async move {
                        let text_resp = match ctx_clone.openlist.fs_get(&target_path).await {
                            Ok(f_info) => {
                                let raw_url = f_info.raw_url.unwrap_or_default();
                                let web_url =
                                    ctx_clone.config.read().await.openlist.openlist_host.clone();
                                let full_url =
                                    format!("{}/#{}", web_url.trim_end_matches('/'), target_path);
                                format!("✅ 任务详情\n\n文件名: {}\n大小: {}\n路径: `{}`\n\n直链: `{}`\n📂 [点击打开下载目录]({})",
                                         md_escape(&filename), md_escape(&size), escape_code(&target_path), escape_code(&raw_url), escape_link_url(&full_url))
                            }
                            Err(_) => {
                                let web_url =
                                    ctx_clone.config.read().await.openlist.openlist_host.clone();
                                let full_url =
                                    format!("{}/#{}", web_url.trim_end_matches('/'), target_path);
                                format!("✅ 任务详情\n\n文件名: {}\n大小: {}\n路径: `{}`\n\n📂 [点击打开下载目录]({})",
                                         md_escape(&filename), md_escape(&size), escape_code(&target_path), escape_link_url(&full_url))
                            }
                        };
                        let keyboard =
                            InlineKeyboardMarkup::new(vec![vec![InlineKeyboardButton::callback(
                                "⬅️ 返回列表",
                                "ods_page_1",
                            )]]);
                        let _ = bot_clone
                            .edit_message_text(chat_id, msg.id, text_resp)
                            .reply_markup(keyboard)
                            .parse_mode(teloxide::types::ParseMode::MarkdownV2)
                            .disable_web_page_preview(true)
                            .await;
                    });
                }
            } else {
                bot.edit_message_text(chat_id, msg.id, "任务详情已失效")
                    .await?;
            }
        }
        return Ok(());
    }

    // Config Manager callbacks
    if data.starts_with("cf_") {
        if data == "cf_back_menu" {
            let keyboard = vec![
                vec![InlineKeyboardButton::callback(
                    "🔧 修改下载设置",
                    "cf_edit_download",
                )],
                vec![InlineKeyboardButton::callback(
                    "📋 查看全部配置",
                    "cf_view_all",
                )],
                vec![InlineKeyboardButton::callback(
                    "✏️ 修改配置项",
                    "cf_edit_item",
                )],
            ];
            bot.edit_message_text(chat_id, msg.id, "⚙️ 配置管理\n\n请选择操作:")
                .reply_markup(InlineKeyboardMarkup::new(keyboard))
                .await?;
        } else if data == "cf_view_all" {
            let cfg = ctx.config.read().await;

            let mut info_text = "📋 全部配置:\n\n".to_string();
            info_text.push_str("【OpenList】\n");
            info_text.push_str(&format!("• 地址: {}\n", cfg.openlist.openlist_host));
            info_text.push_str(&format!("• 默认下载路径: {}\n", cfg.openlist.download_path));
            info_text.push_str(&format!(
                "• 默认下载工具: {}\n\n",
                cfg.openlist.download_tool
            ));

            info_text.push_str("【代理】\n");
            if let Some(p) = &cfg.proxy {
                info_text.push_str(&format!("• 启用: {}\n", if p.enable { "是" } else { "否" }));
                if p.enable {
                    info_text.push_str(&format!("• 地址: {}:{}\n", p.hostname, p.port));
                }
            } else {
                info_text.push_str("• 未配置\n");
            }

            let keyboard = InlineKeyboardMarkup::new(vec![vec![InlineKeyboardButton::callback(
                "🔙 返回",
                "cf_back_menu",
            )]]);
            bot.edit_message_text(chat_id, msg.id, info_text)
                .reply_markup(keyboard)
                .await?;
        } else if data == "cf_edit_item" {
            let keyboard = build_config_edit_menu().await;
            bot.edit_message_text(chat_id, msg.id, "✏️ 选择要修改的配置项:")
                .reply_markup(keyboard)
                .await?;
        } else if data.starts_with("cf_select_") {
            let key = data.strip_prefix("cf_select_").unwrap_or(&data).to_string();
            let mut name = key.as_str();
            for &(k, n) in CONFIG_ITEMS {
                if k == key {
                    name = n;
                    break;
                }
            }

            let mut states = ctx.user_states.lock().await;
            states.insert(
                state_key,
                UserState::ConfigEdit {
                    step: CfStep::EditConfig,
                    message_id: Some(msg.id),
                    tool: None,
                    path: None,
                    config_key: Some(key.clone()),
                    config_name: Some(name.to_string()),
                },
            );
            drop(states);

            let keyboard = InlineKeyboardMarkup::new(vec![vec![InlineKeyboardButton::callback(
                "❌ 取消",
                "cf_back_menu",
            )]]);
            bot.edit_message_text(
                chat_id,
                msg.id,
                format!("✏️ 修改 {}\n\n请直接回复新的值（发送文本消息）", name),
            )
            .reply_markup(keyboard)
            .await?;
        } else if data == "cf_edit_download" {
            match ctx.openlist.get_offline_download_tools().await {
                Ok(tools) => {
                    let mut buttons = Vec::new();
                    for t in tools {
                        let tool_id = register_path(&ctx, &t).await;
                        buttons.push(vec![InlineKeyboardButton::callback(
                            t.clone(),
                            format!("cf_tool_{}", tool_id),
                        )]);
                    }
                    buttons.push(vec![InlineKeyboardButton::callback(
                        "🔙 返回",
                        "cf_back_menu",
                    )]);
                    bot.edit_message_text(chat_id, msg.id, "请选择新的默认下载工具:")
                        .reply_markup(InlineKeyboardMarkup::new(buttons))
                        .await?;
                }
                Err(e) => {
                    bot.send_message(chat_id, format!("获取下载工具失败: {}", e))
                        .await?;
                }
            }
        } else if data.starts_with("cf_tool_") {
            let tool_id = data.strip_prefix("cf_tool_").unwrap_or(&data);
            let selected_tool = get_path(&ctx, tool_id).await.unwrap_or_default();

            // List storage for path
            match ctx.openlist.storage_list().await {
                Ok(storages) => {
                    let mut buttons = Vec::new();
                    for st in storages {
                        if st.disabled {
                            continue;
                        }
                        let name = st
                            .remark
                            .as_deref()
                            .unwrap_or(st.mount_path.as_deref().unwrap_or("/"));
                        let mpath = st.mount_path.as_deref().unwrap_or("/");
                        let path_id = register_path(&ctx, mpath).await;
                        buttons.push(vec![InlineKeyboardButton::callback(
                            format!("📁 {}", name),
                            format!("cf_path_{}", path_id),
                        )]);
                    }
                    buttons.push(vec![InlineKeyboardButton::callback(
                        "✅ 确认选择路径",
                        "cf_confirm_path",
                    )]);

                    {
                        let mut states = ctx.user_states.lock().await;
                        states.insert(
                            state_key,
                            UserState::ConfigEdit {
                                step: CfStep::BrowsingPath,
                                message_id: Some(msg.id),
                                tool: Some(selected_tool.clone()),
                                path: Some("/".to_string()),
                                config_key: None,
                                config_name: None,
                            },
                        );
                    }

                    bot.edit_message_text(
                        chat_id,
                        msg.id,
                        format!(
                            "✅ 已选择工具: {}\n\n请选择存储（点击文件夹进入）:",
                            selected_tool
                        ),
                    )
                    .reply_markup(InlineKeyboardMarkup::new(buttons))
                    .await?;
                }
                Err(e) => {
                    bot.send_message(chat_id, format!("获取存储列表失败: {}", e))
                        .await?;
                }
            }
        } else if data.starts_with("cf_path_") {
            let path_id = data.strip_prefix("cf_path_").unwrap_or(&data).to_string();
            if let Some(cd_path) = get_path(&ctx, &path_id).await {
                let cur_tool = {
                    let states = ctx.user_states.lock().await;
                    match states.get(&state_key) {
                        Some(UserState::ConfigEdit { tool, .. }) => Some(tool.clone()),
                        _ => None,
                    }
                };
                if let Some(cur_tool) = cur_tool {
                    // Browser list
                    match ctx.openlist.fs_list(&cd_path).await {
                        Ok(files) => {
                            let dirs: Vec<_> = files.iter().filter(|f| f.is_dir).collect();
                            let mut buttons = Vec::new();
                            for f in dirs.iter().take(10) {
                                let sub = format!(
                                    "{}/{}",
                                    cd_path.trim_end_matches('/'),
                                    f.name.trim_start_matches('/')
                                );
                                let sub_id = register_path(&ctx, &sub).await;
                                buttons.push(vec![InlineKeyboardButton::callback(
                                    format!("📁 {}", f.name),
                                    format!("cf_dir_{}", sub_id),
                                )]);
                            }
                            buttons.push(vec![InlineKeyboardButton::callback(
                                "✅ 确认此路径",
                                "cf_confirm_path",
                            )]);

                            if cd_path != "/" {
                                let parent = parent_path(&cd_path);
                                let parent_id = register_path(&ctx, &parent).await;
                                buttons.push(vec![InlineKeyboardButton::callback(
                                    "⬅️ 返回上级",
                                    format!("cf_dir_{}", parent_id),
                                )]);
                            }

                            {
                                let mut states = ctx.user_states.lock().await;
                                states.insert(
                                    state_key,
                                    UserState::ConfigEdit {
                                        step: CfStep::BrowsingDir,
                                        message_id: Some(msg.id),
                                        tool: cur_tool,
                                        path: Some(cd_path.clone()),
                                        config_key: None,
                                        config_name: None,
                                    },
                                );
                            }

                            let mut text = format!(
                                "📁 选择存储路径: `{}`\n\n点击目录进入，点击确认此路径按钮完成选择",
                                escape_code(&cd_path)
                            );
                            if dirs.len() > 10 {
                                text.push_str(&format!(
                                    "\n\n⚠️ 目录过多，仅显示前 10 个（共 {} 个）",
                                    dirs.len()
                                ));
                            }
                            bot.edit_message_text(chat_id, msg.id, text)
                                .reply_markup(InlineKeyboardMarkup::new(buttons))
                                .parse_mode(teloxide::types::ParseMode::MarkdownV2)
                                .await?;
                        }
                        Err(e) => {
                            bot.send_message(chat_id, format!("获取目录失败: {}", e))
                                .await?;
                        }
                    }
                }
            }
        } else if data.starts_with("cf_dir_") {
            let path_id = data.strip_prefix("cf_dir_").unwrap_or(&data).to_string();
            if let Some(cd_path) = get_path(&ctx, &path_id).await {
                let cur_tool = {
                    let states = ctx.user_states.lock().await;
                    match states.get(&state_key) {
                        Some(UserState::ConfigEdit { tool, .. }) => Some(tool.clone()),
                        _ => None,
                    }
                };
                if let Some(cur_tool) = cur_tool {
                    // Browser list
                    match ctx.openlist.fs_list(&cd_path).await {
                        Ok(files) => {
                            let dirs: Vec<_> = files.iter().filter(|f| f.is_dir).collect();
                            let mut buttons = Vec::new();
                            for f in dirs.iter().take(10) {
                                let sub = format!(
                                    "{}/{}",
                                    cd_path.trim_end_matches('/'),
                                    f.name.trim_start_matches('/')
                                );
                                let sub_id = register_path(&ctx, &sub).await;
                                buttons.push(vec![InlineKeyboardButton::callback(
                                    format!("📁 {}", f.name),
                                    format!("cf_dir_{}", sub_id),
                                )]);
                            }
                            buttons.push(vec![InlineKeyboardButton::callback(
                                "✅ 确认此路径",
                                "cf_confirm_path",
                            )]);

                            if cd_path != "/" {
                                let parent = parent_path(&cd_path);
                                let parent_id = register_path(&ctx, &parent).await;
                                buttons.push(vec![InlineKeyboardButton::callback(
                                    "⬅️ 返回上级",
                                    format!("cf_dir_{}", parent_id),
                                )]);
                            }

                            {
                                let mut states = ctx.user_states.lock().await;
                                states.insert(
                                    state_key,
                                    UserState::ConfigEdit {
                                        step: CfStep::BrowsingDir,
                                        message_id: Some(msg.id),
                                        tool: cur_tool,
                                        path: Some(cd_path.clone()),
                                        config_key: None,
                                        config_name: None,
                                    },
                                );
                            }

                            let mut text = format!(
                                "📁 选择存储路径: `{}`\n\n点击目录进入，点击确认此路径按钮完成选择",
                                escape_code(&cd_path)
                            );
                            if dirs.len() > 10 {
                                text.push_str(&format!(
                                    "\n\n⚠️ 目录过多，仅显示前 10 个（共 {} 个）",
                                    dirs.len()
                                ));
                            }
                            bot.edit_message_text(chat_id, msg.id, text)
                                .reply_markup(InlineKeyboardMarkup::new(buttons))
                                .parse_mode(teloxide::types::ParseMode::MarkdownV2)
                                .await?;
                        }
                        Err(e) => {
                            bot.send_message(chat_id, format!("获取目录失败: {}", e))
                                .await?;
                        }
                    }
                }
            }
        } else if data == "cf_confirm_path" {
            let confirmed = {
                let mut states = ctx.user_states.lock().await;
                match states.get(&state_key).cloned() {
                    Some(UserState::ConfigEdit {
                        tool: Some(t),
                        path: Some(p),
                        ..
                    }) => {
                        states.remove(&state_key);
                        Some((t, p))
                    }
                    _ => None,
                }
            };

            if let Some((t, p)) = confirmed {
                let save_result = {
                    let mut cfg = ctx.config.write().await;
                    cfg.update_and_save(|candidate| {
                        candidate.openlist.download_tool = t.clone();
                        candidate.openlist.download_path = p.clone();
                    })
                };

                match save_result {
                    Ok(()) => {
                        bot.edit_message_text(chat_id, msg.id, format!("✅ 配置成功！\n\n📋 当前默认配置:\n🔧 下载工具: {}\n📂 下载路径: {}", t, p)).await?;
                    }
                    Err(e) => {
                        bot.send_message(chat_id, format!("❌ 保存失败: {}", e))
                            .await?;
                    }
                }
            }
        }
        return Ok(());
    }

    // File Refresh callbacks
    if data.starts_with("fl_") {
        let resp_text = if data == "fl_refresh_openlist" {
            bot.edit_message_text(chat_id, msg.id, "🔄 正在刷新 OpenList 缓存...")
                .await?;
            run_refresh_openlist(&ctx).await
        } else {
            "".to_string()
        };

        if !resp_text.is_empty() {
            bot.edit_message_text(chat_id, msg.id, resp_text).await?;
        }
        return Ok(());
    }

    Ok(())
}
