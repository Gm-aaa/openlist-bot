use teloxide::types::{InlineKeyboardButton, InlineKeyboardMarkup};

pub const CONFIG_ITEMS: &[(&str, &str)] = &[
    ("openlist.download_path", "默认下载路径"),
    ("openlist.download_tool", "默认下载工具"),
    ("proxy.enable", "代理启用 (true/false)"),
    ("proxy.hostname", "代理地址"),
    ("proxy.port", "代理端口"),
];

pub async fn build_config_edit_menu() -> InlineKeyboardMarkup {
    let mut buttons = Vec::new();
    for &(key, name) in CONFIG_ITEMS {
        buttons.push(vec![InlineKeyboardButton::callback(name, format!("cf_select_{}", key))]);
    }
    buttons.push(vec![InlineKeyboardButton::callback("🔙 返回", "cf_back_menu")]);
    InlineKeyboardMarkup::new(buttons)
}
