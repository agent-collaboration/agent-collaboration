//! Picker display text.
use unicode_width::UnicodeWidthStr;

pub(in crate::session_picker) fn fit_line(line: &str, width: usize) -> String {
    let line = line.replace('\n', " ");
    if UnicodeWidthStr::width(line.as_str()) <= width {
        return line;
    }
    if width == 0 {
        return String::new();
    }
    if width == 1 {
        return ".".to_owned();
    }
    let prefix_budget = (width - 1) / 2;
    let suffix_budget = width - 1 - prefix_budget;
    let mut prefix = String::new();
    for character in line.chars() {
        prefix.push(character);
        if UnicodeWidthStr::width(prefix.as_str()) > prefix_budget {
            prefix.pop();
            break;
        }
    }
    let mut suffix = String::new();
    for character in line.chars().rev() {
        let candidate = format!("{character}{suffix}");
        if UnicodeWidthStr::width(candidate.as_str()) > suffix_budget {
            break;
        }
        suffix = candidate;
    }
    format!("{prefix}.{suffix}")
}

pub(super) fn compact_age(value: &str) -> String {
    value
        .strip_suffix(" ago")
        .or_else(|| value.strip_prefix("in "))
        .unwrap_or(value)
        .to_owned()
}
