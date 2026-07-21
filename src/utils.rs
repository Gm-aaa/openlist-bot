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

/// Escape a string for use inside a MarkdownV2 code span (`` `...` ``).
/// Only `` ` `` and `\` need escaping inside code entities.
pub fn escape_code(text: &str) -> String {
    text.replace('\\', "\\\\").replace('`', "\\`")
}

/// Escape a URL for use inside a MarkdownV2 link destination `(...)`.
/// Only `)` and `\` need escaping there.
pub fn escape_link_url(url: &str) -> String {
    url.replace('\\', "\\\\").replace(')', "\\)")
}

/// Return the parent directory of an absolute path.
///
/// e.g. `/a/b/c` -> `/a/b`, `/a` -> `/`, `/` -> `/`.
pub fn parent_path(path: &str) -> String {
    let trimmed = path.trim_end_matches('/');
    match trimmed.rfind('/') {
        Some(0) | None => "/".to_string(),
        Some(idx) => trimmed[..idx].to_string(),
    }
}

/// Return true if `path` is `root` itself or a descendant of `root`.
///
/// Compares by path segments, so `/movies` is NOT considered within `/movie`
/// (a plain `starts_with` would wrongly say it is).
pub fn path_is_within(path: &str, root: &str) -> bool {
    let path = path.trim_end_matches('/');
    let root = root.trim_end_matches('/');
    if root.is_empty() {
        // root is "/" (or ""): everything is within it.
        return true;
    }
    path == root || path.starts_with(&format!("{}/", root))
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
