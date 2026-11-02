use super::{MAX_PUSH_LINE_BYTES, ParsedPushLine, PushKind, RouterLink, parse_push_line_header};

/// Recognizes complete Router deliveries, rather than display-only title prefixes.
#[must_use]
pub fn parse_push_line(text: &str) -> Option<ParsedPushLine> {
    if text.len() > MAX_PUSH_LINE_BYTES || text.chars().any(char::is_control) {
        return None;
    }
    // A clipped preview keeps the link after the renderer's expansion prompt.
    let (content, link) =
        if let Some((content, link)) = text.rsplit_once(" 📖 Load full message → ") {
            let (preview, omitted) = content.rsplit_once(" (+")?;
            let omitted = omitted.strip_suffix(')')?.parse::<usize>().ok()?;
            if omitted == 0 || !preview.contains(" · \"") || !preview.ends_with('"') {
                return None;
            }
            (content, link)
        } else {
            text.rsplit_once(" · ")?
        };
    let link = RouterLink::parse(link).ok()?;
    let header = parse_push_line_header(content)?;
    let first_section = content.split(" · ").next()?;
    let (prefix, machine) = first_section.rsplit_once(" @")?;
    let machine = match header.kind {
        PushKind::DirectMessage => machine.strip_suffix(" → you")?,
        PushKind::Wake
        | PushKind::ScheduleRun
        | PushKind::Approval
        | PushKind::Question
        | PushKind::SubscriptionActivity
        | PushKind::SubscriptionExpiry
        | PushKind::BoardNotification => machine,
    };
    if prefix.is_empty() || machine.is_empty() {
        return None;
    }
    Some(ParsedPushLine {
        kind: header.kind,
        link,
    })
}

#[cfg(test)]
#[path = "push_line_parse_tests.rs"]
mod tests;
