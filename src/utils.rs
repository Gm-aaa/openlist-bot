use crate::config::Config;

pub fn md_escape(text: &str) -> String {
    let mut escaped = String::new();
    for c in text.chars() {
        if matches!(c, '_' | '*' | '[' | ']' | '(' | ')' | '~' | '`' | '>' | '#' | '+' | '-' | '=' | '|' | '{' | '}' | '.' | '!') {
            escaped.push('\\');
        }
        escaped.push(c);
    }
    escaped
}

pub fn format_size(size: i64) -> String {
    let size_f = size as f64;
    let units = ["B", "KB", "MB", "GB", "TB", "PB"];
    let mut current_size = size_f;
    for unit in units {
        if current_size < 1024.0 {
            return format!("{:.1}{}", current_size, unit);
        }
        current_size /= 1024.0;
    }
    format!("{:.1}PB", current_size)
}

pub fn is_admin(user_id: i64, config: &Config) -> bool {
    user_id == config.user.admin
}

pub fn is_member(chat_id: i64, user_id: i64, config: &Config) -> bool {
    if user_id == config.user.admin {
        return true;
    }
    if config.user.member.is_empty() {
        return true;
    }
    config.user.member.contains(&chat_id) || config.user.member.contains(&user_id)
}
