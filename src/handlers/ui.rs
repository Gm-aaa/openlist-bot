use teloxide::types::InlineKeyboardButton;

pub fn title_bar(title: &str) -> String {
    format!("━━━ {} ━━━\n\n", title)
}

pub fn meta_line(page: usize, total_pages: usize, total_items: usize, extra: &str) -> String {
    let extra_part = if extra.is_empty() {
        String::new()
    } else {
        format!(" · {}", extra)
    };
    format!("第 {}/{} 页 · 共 {} 项{}\n\n", page, total_pages, total_items, extra_part)
}


pub fn page_nav(page: usize, total_pages: usize, prefix: &str) -> Option<Vec<InlineKeyboardButton>> {
    if total_pages <= 1 {
        return None;
    }
    let mut row = Vec::new();
    if page > 1 {
        row.push(InlineKeyboardButton::callback("⬅️ 上一页", format!("{}_prev", prefix)));
    }
    row.push(InlineKeyboardButton::callback(
        format!("{}/{}", page, total_pages),
        format!("{}_page", prefix),
    ));
    if page < total_pages {
        row.push(InlineKeyboardButton::callback("➡️ 下一页", format!("{}_next", prefix)));
    }
    Some(row)
}

pub fn dir_label(name: &str) -> String {
    let truncated = if name.chars().count() > 16 {
        name.chars().take(13).collect::<String>() + "..."
    } else {
        name.to_string()
    };
    format!("📁 {}", truncated)
}

pub fn file_label(name: &str, size: &str) -> String {
    let truncated = if name.chars().count() > 12 {
        name.chars().take(9).collect::<String>() + "..."
    } else {
        name.to_string()
    };
    format!("📄 {} · {}", truncated, size)
}

#[allow(dead_code)]
pub fn back_button(prefix: &str) -> Vec<InlineKeyboardButton> {
    vec![InlineKeyboardButton::callback("↩️ 返回", prefix.to_string())]
}

#[allow(dead_code)]
pub fn close_button(prefix: &str) -> Vec<InlineKeyboardButton> {
    vec![InlineKeyboardButton::callback("❌ 关闭", prefix.to_string())]
}
