use std::sync::Arc;
use teloxide::prelude::*;
use teloxide::types::{InlineKeyboardButton, InlineKeyboardMarkup};
use tracing::{info, error};

use crate::BotContext;
use crate::api::openlist::FileItem;
use crate::handlers::ui;
use crate::utils::{is_admin, format_size};

pub const PER_PAGE: usize = 10;

pub async fn handle_st(bot: Bot, msg: Message, ctx: Arc<BotContext>) -> ResponseResult<()> {
    let chat_id = msg.chat.id;
    let user_id = msg.from().map_or(0, |u| u.id.0 as i64);

    let cfg = ctx.config.read().await;
    if !is_admin(user_id, &cfg) {
        return Ok(());
    }

    info!("st_command invoked");
    if let Err(e) = ctx.openlist.login().await {
        error!("OpenList login failed: {}", e);
        bot.send_message(chat_id, format!("登录 OpenList 失败: {}", e)).await?;
        return Ok(());
    }

    match ctx.openlist.storage_list().await {
        Ok(storages) => {
            if storages.is_empty() {
                bot.send_message(chat_id, "暂无存储").await?;
                return Ok(());
            }

            let mut buttons = Vec::new();
            for storage in storages {
                let remark = storage.remark.as_deref().unwrap_or("");
                let mount = storage.mount_path.as_deref().unwrap_or("/");
                let name = if !remark.is_empty() { remark } else { mount };
                let status = if storage.disabled { "❌" } else { "✅" };

                // Register the mount path so the callback data stays a short
                // numeric id — raw (e.g. CJK) mount paths can blow past
                // Telegram's 64-byte callback_data limit and make the whole
                // storage list fail to send.
                let mount_id = crate::register_path(&ctx, mount).await;
                buttons.push(vec![InlineKeyboardButton::callback(
                    format!("{}{}", status, name),
                    format!("storage_{}:{}", storage.id, mount_id),
                )]);
            }
            buttons.push(vec![InlineKeyboardButton::callback("❌ 取消", "st_cancel")]);

            bot.send_message(chat_id, ui::title_bar("📂 浏览存储") + "选择存储:")
                .reply_markup(InlineKeyboardMarkup::new(buttons))
                .await?;
        }
        Err(e) => {
            bot.send_message(chat_id, format!("获取存储列表失败: {}", e)).await?;
        }
    }

    Ok(())
}

pub async fn build_file_list(
    ctx: &BotContext,
    content: &[FileItem],
    current_path: &str,
    page: usize,
) -> (String, InlineKeyboardMarkup) {
    let total_items = content.len();
    let total_pages = if total_items == 0 { 1 } else { total_items.div_ceil(PER_PAGE) };
    let page = page.clamp(1, total_pages);

    let start = (page - 1) * PER_PAGE;
    let end = (start + PER_PAGE).min(total_items);
    let page_items = &content[start..end];

    let mut text = ui::title_bar("📂 浏览存储");
    text.push_str(&format!("📁 `{}`\n", current_path));
    text.push_str(&ui::meta_line(page, total_pages, total_items, ""));

    let mut buttons = Vec::new();
    for item in page_items {
        let name = &item.name;
        let base = current_path.trim_end_matches('/');
        let full_path = format!("{}/{}", base, name);
        let path_id = crate::register_path(ctx, &full_path).await;

        let label = if item.is_dir {
            ui::dir_label(name)
        } else {
            ui::file_label(name, &format_size(item.size))
        };
        let action = if item.is_dir {
            format!("cd_{}", path_id)
        } else {
            format!("file_{}", path_id)
        };

        buttons.push(vec![
            InlineKeyboardButton::callback(label, action),
            InlineKeyboardButton::callback("🗑️", format!("del_{}", path_id)),
        ]);
    }

    // Page navigation
    if let Some(nav) = ui::page_nav(page, total_pages, "browse") {
        buttons.push(nav);
    }

    buttons.push(vec![
        InlineKeyboardButton::callback("📁+ 新建文件夹", "st_mkdir"),
        InlineKeyboardButton::callback("📤 上传文件", "st_upload"),
    ]);

    if current_path != "/" && !current_path.is_empty() {
        buttons.push(vec![InlineKeyboardButton::callback("⬅️ 返回上一级", "back")]);
    }

    (text, InlineKeyboardMarkup::new(buttons))
}
