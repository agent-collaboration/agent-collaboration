//! Typed identity content lets shared chrome preserve a whole tag while fitting text.
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct PinnedContext {
    pub text: String,
    /// The raw tag. Shared chrome supplies its brackets.
    pub tag: Option<String>,
}
impl PinnedContext {
    pub fn new(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            tag: None,
        }
    }
    pub fn with_tag(mut self, tag: impl Into<String>) -> Self {
        self.tag = Some(tag.into());
        self
    }
    pub(super) fn natural_width(&self) -> usize {
        self.text.width()
            + self.tag.as_ref().map_or(0, |tag| {
                tag.width() + 2 + usize::from(!self.text.is_empty())
            })
    }
    pub(super) fn fitted(&self, width: usize) -> (String, Option<String>) {
        let tag = self.tag.as_ref().map(|tag| format!("[{tag}]"));
        let tag_width = tag.as_ref().map_or(0, |tag| tag.width());
        let gap = usize::from(tag_width > 0 && !self.text.is_empty() && tag_width < width);
        let text = fit_pinned_text(&self.text, width.saturating_sub(tag_width + gap));
        (text, tag.map(|tag| fit_pinned_text(&tag, width)))
    }
}

pub(super) fn fit_pinned_text(content: &str, width: usize) -> String {
    if content.width() <= width {
        return content.to_owned();
    }
    if width == 0 {
        return String::new();
    }
    let mut result = String::new();
    let mut used = 0;
    for character in content.chars() {
        let cells = character.width().unwrap_or(0);
        if used + cells >= width {
            break;
        }
        result.push(character);
        used += cells;
    }
    result.push('…');
    result
}
