use teloxide::types::{InlineKeyboardButton, InlineKeyboardMarkup};
use crate::UserState;

pub async fn build_download_confirm_message(info: &UserState) -> (String, InlineKeyboardMarkup) {
    if let UserState::TorrentDownloadSelect { file_name, path, tool, index, cmid, .. } = info {
        let keyboard = vec![
            vec![InlineKeyboardButton::callback(format!("📂 路径: {}", path), "sb_path_select")],
            vec![InlineKeyboardButton::callback(format!("🔧 工具: {}", tool), "sb_tool_select")],
            vec![InlineKeyboardButton::callback("✅ 确认下载", format!("tb_add_{}_{}", index, cmid))],
        ];
        let text = format!("📥 添加到离线下载\n\n📄 文件: `{}`\n📂 路径: {}\n🔧 工具: {}\n🔗 链接: 磁力链接",
                           file_name, path, tool);
        (text, InlineKeyboardMarkup::new(keyboard))
    } else {
        ("".to_string(), InlineKeyboardMarkup::default())
    }
}
