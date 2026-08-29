use std::borrow::Cow;

pub const PATH_SEPARATOR: char = std::path::MAIN_SEPARATOR;
pub const PATH_SEPARATOR_STR: &str = std::path::MAIN_SEPARATOR_STR;
pub const INVERTED_PATH_SEPARATOR: char = if PATH_SEPARATOR == '/' { '\\' } else { '/' };

pub fn normalize_path(glob: &str) -> Cow<'_, str> {
    let mut value: Cow<str> = Cow::Borrowed(glob);
    if value.contains(INVERTED_PATH_SEPARATOR) {
        value = Cow::Owned(value.replace(INVERTED_PATH_SEPARATOR, PATH_SEPARATOR_STR));
    }
    let duplicated_separator = &format!("{PATH_SEPARATOR}{PATH_SEPARATOR}");
    while value.contains(duplicated_separator) {
        value = Cow::Owned(value.replace(duplicated_separator, PATH_SEPARATOR_STR));
    }
    value
}
