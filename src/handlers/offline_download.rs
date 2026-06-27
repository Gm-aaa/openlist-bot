use std::sync::Arc;
use teloxide::prelude::*;
use teloxide::types::{InlineKeyboardButton, InlineKeyboardMarkup};
use tracing::{info, error};

use percent_encoding;

use crate::BotContext;
use crate::utils::{is_admin, format_size};
use crate::{UserState, OdStep};

pub async fn handle_download(bot: Bot, msg: Message, _ctx: Arc<BotContext>) -> ResponseResult<()> {
    let chat_id = msg.chat.id;

    let keyboard = InlineKeyboardMarkup::new(vec![
        vec![InlineKeyboardButton::callback("➕ 新建下载任务", "od_new")],
        vec![InlineKeyboardButton::callback("📋 查看任务状态", "od_status")],
        vec![InlineKeyboardButton::callback("⚙️ 下载设置", "cf_back_menu")],
    ]);

    bot.send_message(chat_id, "📥 离线下载\n\n请选择操作:")
        .reply_markup(keyboard)
        .await?;

    Ok(())
}

pub async fn start_od_download_flow(
    bot: Bot,
    msg: Message,
    ctx: Arc<BotContext>,
    clicker_user_id: Option<i64>,
) -> ResponseResult<()> {
    let chat_id = msg.chat.id;
    let user_id = clicker_user_id.unwrap_or_else(|| msg.from().map_or(0, |u| u.id.0 as i64));

    let cfg = ctx.config.read().await;
    if !is_admin(user_id, &cfg) {
        return Ok(());
    }

    if let Err(e) = ctx.openlist.login().await {
        error!("OpenList login failed: {}", e);
        bot.send_message(chat_id, format!("登录 OpenList 失败: {}", e)).await?;
        return Ok(());
    }

    match ctx.openlist.get_offline_download_tools().await {
        Ok(tools) => {
            if tools.is_empty() {
                bot.send_message(chat_id, "暂无可用的离线下载工具").await?;
                return Ok(());
            }

            let mut buttons = Vec::new();
            for tool in tools {
                buttons.push(vec![InlineKeyboardButton::callback(tool.clone(), format!("od_tool_{}", tool))]);
            }

            let reply = bot.send_message(chat_id, "请选择下载工具:")
                .reply_markup(InlineKeyboardMarkup::new(buttons))
                .await?;

            let mut states = ctx.user_states.lock().await;
            states.insert(chat_id, UserState::OfflineDownload {
                step: OdStep::SelectingTool,
                message_id: Some(reply.id),
                tool: None,
                path: None,
                urls: Vec::new(),
            });
        }
        Err(e) => {
            bot.send_message(chat_id, format!("获取下载工具失败: {}", e)).await?;
        }
    }

    Ok(())
}

pub fn extract_file_info(name: &str) -> (String, String) {
    let mut filename = "未知".to_string();
    let mut target_path = "/".to_string();

    if name.contains("download ") {
        if let Some(pos) = name.find("download ") {
            let part = &name[pos + 9..];
            if let Some(pos_to) = part.find(" to (") {
                filename = part[..pos_to].to_string();
                let path_part = &part[pos_to + 5..];
                target_path = path_part.trim_end_matches(')').to_string();
            }
        }
    } else {
        filename = name.to_string();
    }

    // Parse and URL-decode magnet dn (display name) parameter
    if filename.starts_with("magnet:?") {
        if let Some(dn_pos) = filename.find("dn=") {
            let dn_part = &filename[dn_pos + 3..];
            let end_pos = dn_part.find('&').unwrap_or(dn_part.len());
            let dn_val = &dn_part[..end_pos];
            if let Ok(decoded) = percent_encoding::percent_decode_str(dn_val).decode_utf8() {
                filename = decoded.to_string();
            }
        }
    }

    (filename, target_path)
}

pub async fn handle_ods(
    bot: Bot,
    msg: Message,
    ctx: Arc<BotContext>,
    page: usize,
    clicker_user_id: Option<i64>,
) -> ResponseResult<()> {
    let chat_id = msg.chat.id;
    let user_id = clicker_user_id.unwrap_or_else(|| msg.from().map_or(0, |u| u.id.0 as i64));

    let cfg = ctx.config.read().await;
    if !is_admin(user_id, &cfg) {
        return Ok(());
    }

    if let Err(e) = ctx.openlist.login().await {
        error!("OpenList login failed: {}", e);
        bot.send_message(chat_id, format!("登录 OpenList 失败: {}", e)).await?;
        return Ok(());
    }

    let undone = match ctx.openlist.get_offline_download_undone_task().await {
        Ok(t) => t,
        Err(e) => {
            bot.send_message(chat_id, format!("获取未完成任务失败: {}", e)).await?;
            return Ok(());
        }
    };

    let done = match ctx.openlist.get_offline_download_done_task().await {
        Ok(t) => t,
        Err(e) => {
            bot.send_message(chat_id, format!("获取已完成任务失败: {}", e)).await?;
            return Ok(());
        }
    };

    // Combine all tasks
    let mut all_tasks = Vec::new();
    for mut task in undone {
        task.status = Some("undone".to_string());
        all_tasks.push(task);
    }
    let undone_count = all_tasks.len();

    for mut task in done {
        task.status = Some("done".to_string());
        all_tasks.push(task);
    }
    let done_count = all_tasks.len() - undone_count;

    let total = all_tasks.len();
    let total_pages = if total == 0 { 1 } else { (total + 9) / 10 };
    let page = page.clamp(1, total_pages);

    let start = (page - 1) * 10;
    let end = (start + 10).min(total);
    let page_tasks = &all_tasks[start..end];

    // Build text
    let mut text = "📥 离线下载状态\n".to_string();
    text.push_str("━━━━━━━━━━━━━━━━━━\n");
    text.push_str(&format!("⏳ 进行中: {} 个\n", undone_count));
    text.push_str(&format!("✅ 已完成: {} 个\n", done_count));
    text.push_str("━━━━━━━━━━━━━━━━━━\n");
    text.push_str(&format!("共 {} 个任务 (第 {}/{} 页)\n\n", total, page, total_pages));

    let mut buttons = Vec::new();
    for task in page_tasks {
        let (filename, _) = extract_file_info(&task.name);
        let task_status = task.status.as_deref().unwrap_or("unknown");
        let btn_text = if task_status == "undone" {
            format!("⏳ {} ({:.0}%)", filename, task.progress)
        } else {
            let size_str = format_size(task.total_bytes);
            format!("✅ {} ({})", filename, size_str)
        };

        let short_btn_text = if btn_text.chars().count() > 40 {
            btn_text.chars().take(37).collect::<String>() + "..."
        } else {
            btn_text
        };

        buttons.push(vec![InlineKeyboardButton::callback(short_btn_text, format!("ods_detail_{}", task.id))]);
    }

    // Pagination
    let mut page_buttons = Vec::new();
    if page > 1 {
        page_buttons.push(InlineKeyboardButton::callback("⬅️ 上一页", format!("ods_page_{}", page - 1)));
    }
    if page < total_pages {
        page_buttons.push(InlineKeyboardButton::callback("➡️ 下一页", format!("ods_page_{}", page + 1)));
    }
    if !page_buttons.is_empty() {
        buttons.push(page_buttons);
    }
    buttons.push(vec![InlineKeyboardButton::callback("❌ 关闭", "ods_close")]);

    bot.send_message(chat_id, text)
        .reply_markup(InlineKeyboardMarkup::new(buttons))
        .await?;

    Ok(())
}

pub async fn start_background_notifier(bot: Bot, ctx: Arc<BotContext>) {
    info!("Starting background task monitor loop");
    let mut interval = tokio::time::interval(std::time::Duration::from_secs(30));
    
    // First run initialization flag
    let mut initialized = false;

    loop {
        interval.tick().await;

        let admin_id = {
            let cfg = ctx.config.read().await;
            cfg.user.admin
        };

        if admin_id == 0 {
            continue;
        }

        match ctx.openlist.get_offline_download_done_task().await {
            Ok(done_tasks) => {
                let mut done_ids = ctx.od_done_ids.lock().await;

                // Gather current completed IDs
                let current_done_ids: std::collections::HashSet<String> = done_tasks
                    .iter()
                    .map(|t| t.id.clone())
                    .collect();

                if !initialized {
                    *done_ids = current_done_ids;
                    initialized = true;
                    info!("Completed task monitor initialized with {} existing items", done_ids.len());
                    continue;
                }

                // Check for new done tasks
                let new_dones: Vec<_> = done_tasks
                    .iter()
                    .filter(|t| !done_ids.contains(&t.id))
                    .collect();

                for task in new_dones {
                    let (name, path) = extract_file_info(&task.name);
                    let size = format_size(task.total_bytes);
                    let msg = if let Some(err) = &task.error {
                        if !err.is_empty() {
                            format!("❌ 下载失败\n\n文件: {}\n路径: {}\n错误: {}", name, path, err)
                        } else {
                            format!("✅ 下载完成\n\n文件: {}\n大小: {}\n路径: {}", name, size, path)
                        }
                    } else {
                        format!("✅ 下载完成\n\n文件: {}\n大小: {}\n路径: {}", name, size, path)
                    };

                    info!("Sending download task completion notification for task: {}", task.id);
                    if let Err(e) = bot.send_message(ChatId(admin_id), msg).await {
                        error!("Failed to send completion notification to admin: {}", e);
                    }

                    done_ids.insert(task.id.clone());
                }

                // Sync
                *done_ids = current_done_ids;
            }
            Err(e) => {
                error!("Monitor loop error fetching completed tasks: {}", e);
            }
        }
    }
}
