use std::sync::Arc;
use teloxide::prelude::*;
use teloxide::types::{InlineKeyboardButton, InlineKeyboardMarkup, MessageId};
use tracing::info;

use crate::BotContext;
use crate::api::pansou::PanSouResult;
use crate::handlers::ui;
use crate::utils::{is_member, md_escape};

const PER_PAGE: usize = 5;

#[derive(Clone, Debug)]
pub struct PanSouPage {
    pub all_results: Vec<PanSouResult>,
    pub keyword: String,
    pub cmid: String,
    pub filter_type: Option<String>,
    pub index: usize,
}

impl PanSouPage {
    pub fn new(results: Vec<PanSouResult>, keyword: String, cmid: String) -> Self {
        Self {
            all_results: results,
            keyword,
            cmid,
            filter_type: None,
            index: 0,
        }
    }

    pub fn filtered_results(&self) -> Vec<PanSouResult> {
        match &self.filter_type {
            Some(t) => self.all_results.iter().filter(|r| &r.pan_type == t).cloned().collect(),
            None => self.all_results.clone(),
        }
    }

    pub fn page_count(&self) -> usize {
        let count = self.filtered_results().len();
        if count == 0 {
            1
        } else {
            (count + PER_PAGE - 1) / PER_PAGE
        }
    }

    pub fn get_text_with_info(&self) -> String {
        let filtered = self.filtered_results();
        let total = filtered.len();
        let page = self.index + 1;
        let total_pages = self.page_count();

        let start = self.index * PER_PAGE;
        let end = (start + PER_PAGE).min(total);
        let items = &filtered[start..end];

        let mut text = ui::title_bar("🔍 网盘搜索");

        let filter_label = match &self.filter_type {
            Some(t) => get_pan_emoji(t),
            None => "",
        };
        text.push_str(&ui::meta_line(page, total_pages, total, filter_label));

        for (i, item) in items.iter().enumerate() {
            let index = start + i + 1;
            let icon = if item.source == "Sukebei" {
                "🔞"
            } else {
                get_pan_emoji(&item.pan_type)
            };

            let mut password = item.password.clone();
            if password.is_empty() && item.url.contains("pwd=") {
                if let Some(pos) = item.url.find("pwd=") {
                    let pwd_part = &item.url[pos + 4..];
                    let end_pos = pwd_part.find('&').unwrap_or(pwd_part.len());
                    password = pwd_part[..end_pos].to_string();
                }
            }

            let is_magnet = item.pan_type == "magnet" || item.pan_type == "ed2k";
            let clean_url = if is_magnet {
                item.url.as_str()
            } else {
                item.url.split('#').next().unwrap_or("")
            };
            let name_escaped = md_escape(&item.name);

            // Format list item using bold/hyperlinks
            text.push_str(&format!("{}\\. {} ", index, icon));
            if !is_magnet && !clean_url.is_empty() {
                text.push_str(&format!("[*{}*]({})\n", name_escaped, clean_url));
            } else {
                text.push_str(&format!("*{}*\n", name_escaped));
            }

            let mut detail = item.size.clone();
            if !password.is_empty() {
                if !detail.is_empty() {
                    detail.push_str(" · ");
                }
                detail.push_str(&format!("密码: {}", password));
            }
            if is_magnet && !clean_url.is_empty() {
                if !detail.is_empty() {
                    detail.push_str(" · ");
                }
                detail.push_str("🧲 磁力链请用下方复制按钮");
            }

            let clean_detail = detail.replace('\\', "\\\\").replace('`', "\\`");
            if !clean_detail.is_empty() {
                text.push_str(&format!("   `{}`\n", clean_detail));
            }
            
            text.push('\n');
        }

        text
    }

    pub fn btn(&self) -> InlineKeyboardMarkup {
        let filtered = self.filtered_results();
        let total = filtered.len();
        let start = self.index * PER_PAGE;
        let end = (start + PER_PAGE).min(total);
        let items = &filtered[start..end];

        let mut keyboard = Vec::new();

        // 1. Action buttons for items on the current page
        let mut web_buttons = Vec::new();
        for (i, item) in items.iter().enumerate() {
            let count = start + i + 1;
            
            let source_name = match item.pan_type.as_str() {
                "baidu" => "百度",
                "aliyun" => "阿里",
                "quark" => "夸克",
                "tianyi" => "天翼",
                "115" => "115",
                "pikpak" => "PikPak",
                "xunlei" => "迅雷",
                "123" => "123",
                "uc" => "UC",
                "magnet" => "磁力",
                "ed2k" => "电驴",
                _ => "链接",
            };

            if (item.pan_type == "magnet" || item.pan_type == "ed2k") && !item.url.is_empty() {
                if let Some(global_idx) = self.all_results.iter().position(|r| r.url == item.url) {
                    keyboard.push(vec![
                        InlineKeyboardButton::callback(
                            format!("📥 #{} 下载", count),
                            format!("s_dl_{}_{}", self.cmid, global_idx),
                        ),
                        InlineKeyboardButton::callback(
                            format!("📋 #{} 复制", count),
                            format!("s_cp_{}_{}", self.cmid, global_idx),
                        )
                    ]);
                }
            } else if !item.url.is_empty() {
                if let Ok(url) = url::Url::parse(&item.url) {
                    web_buttons.push(InlineKeyboardButton::url(
                        format!("🔗 #{} {}", count, source_name),
                        url,
                    ));
                }
            }
        }

        for chunk in web_buttons.chunks(2) {
            keyboard.push(chunk.to_vec());
        }

        // 2. Filter buttons
        let mut pan_types: Vec<String> = self.all_results.iter().map(|r| r.pan_type.clone()).collect();
        pan_types.sort();
        pan_types.dedup();

        let mut filter_row = vec![InlineKeyboardButton::callback("🌐 全部", "search_filter_all")];
        for pt in pan_types.iter().take(5) {
            filter_row.push(InlineKeyboardButton::callback(get_pan_emoji(pt), format!("search_filter_{}", pt)));
        }
        keyboard.push(filter_row);

        if pan_types.len() > 5 {
            let mut row2 = Vec::new();
            for pt in pan_types.iter().skip(5).take(5) {
                row2.push(InlineKeyboardButton::callback(get_pan_emoji(pt), format!("search_filter_{}", pt)));
            }
            if !row2.is_empty() {
                keyboard.push(row2);
            }
        }

        // 3. Page navigation
        let total_pages = self.page_count();
        if let Some(nav) = ui::page_nav(self.index + 1, total_pages, "search") {
            keyboard.push(nav);
        }

        InlineKeyboardMarkup::new(keyboard)
    }
}

pub fn get_pan_emoji(pan_type: &str) -> &'static str {
    match pan_type {
        "baidu" => "🔵百度",
        "aliyun" => "🟢阿里",
        "quark" => "🟣夸克",
        "tianyi" => "🔴天翼",
        "115" => "🟠115",
        "pikpak" => "⚡PikPak",
        "xunlei" => "⚙️迅雷",
        "123" => "🔶123",
        "magnet" => "🧲磁力",
        "ed2k" => "📎电驴",
        "uc" => "🌐UC",
        _ => "📦",
    }
}

pub async fn build_search_config_menu(ctx: &BotContext) -> (String, InlineKeyboardMarkup) {
    let cfg = ctx.config.read().await;
    let allowed = &cfg.search.allowed_sources;

    let mut text = ui::title_bar("⚙️ 搜索源配置");
    text.push_str("请在下方配置需要启用的搜索结果源。\n\n");
    text.push_str("💡 *说明*：\n");
    text.push_str("• ❌ 代表禁用，✅ 代表启用\n");
    text.push_str("• 搜索结果仅包含选中的源，底部的筛选按钮也仅展示已启用的源\n");

    let all_sources = vec![
        ("baidu", "🔵 百度"),
        ("aliyun", "🟢 阿里"),
        ("quark", "🟣 夸克"),
        ("tianyi", "🔴 天翼"),
        ("115", "🟠 115"),
        ("pikpak", "⚡ PikPak"),
        ("xunlei", "⚙️ 迅雷"),
        ("123", "🔶 123"),
        ("uc", "🌐 UC"),
        ("magnet", "🧲 磁力"),
        ("ed2k", "📎 电驴"),
        ("sukebei", "🔞 AV磁力"),
    ];

    let mut buttons = Vec::new();
    let mut row = Vec::new();

    for &(key, name) in &all_sources {
        let is_enabled = allowed.contains(&key.to_string());
        let status_emoji = if is_enabled { "✅" } else { "❌" };
        let label = format!("{} {}", status_emoji, name);
        let callback = format!("cfg_src_toggle_{}", key);
        
        row.push(InlineKeyboardButton::callback(label, callback));
        if row.len() == 2 {
            buttons.push(row.clone());
            row.clear();
        }
    }
    if !row.is_empty() {
        buttons.push(row);
    }

    buttons.push(vec![InlineKeyboardButton::callback("🔍 开始搜索资源", "cfg_src_start_search")]);
    buttons.push(vec![InlineKeyboardButton::callback("❌ 关闭", "cfg_src_close")]);

    (text, InlineKeyboardMarkup::new(buttons))
}

pub async fn handle_s(bot: Bot, msg: Message, query: String, ctx: Arc<BotContext>) -> ResponseResult<()> {
    handle_s_with_edit(bot, msg, query, ctx, None).await
}

pub async fn handle_s_with_edit(
    bot: Bot,
    msg: Message,
    query: String,
    ctx: Arc<BotContext>,
    edit_msg_id: Option<MessageId>,
) -> ResponseResult<()> {
    let chat_id = msg.chat.id;
    let user_id = msg.from().map_or(0, |u| u.id.0 as i64);

    let cfg = ctx.config.read().await;
    if !is_member(chat_id.0, user_id, &cfg) {
        return Ok(());
    }

    if query.trim().is_empty() {
        let (text, keyboard) = build_search_config_menu(&ctx).await;
        if let Some(mid) = edit_msg_id {
            bot.edit_message_text(chat_id, mid, text)
                .reply_markup(keyboard)
                .await?;
        } else {
            bot.send_message(chat_id, text)
                .reply_markup(keyboard)
                .await?;
        }
        return Ok(());
    }

    let search_msg = if let Some(mid) = edit_msg_id {
        bot.edit_message_text(chat_id, mid, "⏳ 正在搜索中...").await?
    } else {
        bot.send_message(chat_id, "⏳ 正在搜索中...").await?
    };

    let is_sukebei_enabled = {
        let cfg = ctx.config.read().await;
        cfg.search.allowed_sources.contains(&"sukebei".to_string())
    };

    let pansou_fut = ctx.pansou.search(&query);

    let (pansou_res, sukebei_res) = if is_sukebei_enabled {
        let sukebei_fut = ctx.pansou.search_sukebei(&query);
        let (p, s) = tokio::join!(pansou_fut, sukebei_fut);
        (p, Some(s))
    } else {
        (pansou_fut.await, None)
    };

    let allowed = {
        let cfg = ctx.config.read().await;
        cfg.search.allowed_sources.clone()
    };

    let mut filtered_results = Vec::new();

    match pansou_res {
        Ok(results) => {
            for r in results {
                if allowed.contains(&r.pan_type) {
                    filtered_results.push(r);
                }
            }
        }
        Err(e) => {
            info!("PanSou search failed: {}", e);
        }
    }

    if let Some(Ok(results)) = sukebei_res {
        for r in results {
            // Sukebei results are always of type "magnet" which is allowed under "sukebei"
            filtered_results.push(r);
        }
    }

    if filtered_results.is_empty() {
        bot.edit_message_text(chat_id, search_msg.id, "未搜索到包含已启用源的资源，请换个关键词或启用更多结果源").await?;
        return Ok(());
    }

            let cmid = format!("{}|{}", chat_id, search_msg.id);
            
            // Populate caches
            {
                let mut results_cache = ctx.pansou_results.lock().await;
                // Delete old caches for this cmid
                results_cache.retain(|k, _| !k.starts_with(&format!("{}_", cmid)));
                for (i, item) in filtered_results.iter().enumerate() {
                    results_cache.insert(format!("{}_{}", cmid, i), item.clone());
                }
            }

            let page = PanSouPage::new(filtered_results, query, cmid.clone());
            let text = page.get_text_with_info();
            let keyboard = page.btn();

            {
                let mut pages_cache = ctx.pansou_pages.lock().await;
                pages_cache.insert(cmid, page);
            }

            bot.edit_message_text(chat_id, search_msg.id, text)
                .reply_markup(keyboard)
                .parse_mode(teloxide::types::ParseMode::MarkdownV2)
                .disable_web_page_preview(true)
                .await?;
            
            Ok(())
}
