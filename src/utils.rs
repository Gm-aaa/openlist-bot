use crate::config::Config;

pub fn md_escape(text: &str) -> String {
    let mut escaped = String::new();
    for c in text.chars() {
        if matches!(
            c,
            '_' | '*'
                | '['
                | ']'
                | '('
                | ')'
                | '~'
                | '`'
                | '>'
                | '#'
                | '+'
                | '-'
                | '='
                | '|'
                | '{'
                | '}'
                | '.'
                | '!'
        ) {
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

/// Return true when `path` is an immediate child of `current` and both stay
/// inside the selected storage root. This rejects callbacks left over from a
/// different directory or storage session.
pub fn path_is_direct_child(path: &str, current: &str, root: &str) -> bool {
    let current = current.trim_end_matches('/');
    let current = if current.is_empty() { "/" } else { current };
    path_is_within(path, root) && parent_path(path) == current
}

/// Validate a single path component supplied by a user.
pub fn is_valid_path_component(name: &str) -> bool {
    !name.is_empty()
        && name != "."
        && name != ".."
        && !name.contains('/')
        && !name.contains('\\')
        && !name.contains('\0')
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

/// Whether `user_id` (in `chat_id`) may use member-level features (search,
/// paging, copying links).
///
/// ⚠️ FOOTGUN: an **empty** `member` list means *everyone* is treated as a
/// member — i.e. the search feature is fully public to anyone who can reach the
/// bot (including every member of any group it's in). This is intentional
/// "open by default", but if you meant to restrict access you MUST populate
/// `user.member` in config.yaml with the allowed user IDs (and/or group chat
/// IDs). A whitelisted group chat_id grants access to *all* of that group's
/// members.
pub fn is_member(chat_id: i64, user_id: i64, config: &Config) -> bool {
    if user_id == config.user.admin {
        return true;
    }
    if config.user.member.is_empty() {
        return true;
    }
    config.user.member.contains(&chat_id) || config.user.member.contains(&user_id)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn direct_child_must_belong_to_current_storage_directory() {
        assert!(path_is_direct_child("/disk/a/file", "/disk/a", "/disk"));
        assert!(!path_is_direct_child("/other/file", "/disk/a", "/disk"));
        assert!(!path_is_direct_child("/disk/b/file", "/disk/a", "/disk"));
        assert!(!path_is_direct_child(
            "/disk/a/nested/file",
            "/disk/a",
            "/disk"
        ));
    }

    #[test]
    fn user_supplied_directory_name_is_one_component() {
        assert!(is_valid_path_component("新目录"));
        for invalid in ["", ".", "..", "a/b", "a\\b", "bad\0name"] {
            assert!(!is_valid_path_component(invalid), "accepted {invalid:?}");
        }
    }
}
