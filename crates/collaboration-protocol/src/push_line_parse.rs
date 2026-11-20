use super::{MAX_PUSH_LINE_BYTES, ParsedPushLine, ParsedPushLineHeader, PushKind, RouterLink};

/// Recognizes complete Router deliveries, rather than display-only title prefixes.
#[must_use]
pub fn parse_push_line(text: &str) -> Option<ParsedPushLine> {
    if text.len() > MAX_PUSH_LINE_BYTES || text.chars().any(char::is_control) {
        return None;
    }
    // A clipped preview keeps the link after the expansion prompt.
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

#[must_use]
pub fn escape_push_line_field(value: &str) -> String {
    let mut escaped = String::new();
    for character in value.chars() {
        match character {
            '\\' => escaped.push_str("\\\\"),
            '"' => escaped.push_str("\\\""),
            '\r' | '\n' | '\u{2028}' | '\u{2029}' => escaped.push('⏎'),
            '·' => escaped.push_str("\\u{b7}"),
            character if character.is_control() => {
                use std::fmt::Write as _;
                let _ = write!(escaped, "\\u{{{:x}}}", u32::from(character));
            }
            character => escaped.push(character),
        }
    }
    escaped
}

/// Recognizes the display header of a historical Router delivery.
#[must_use]
pub fn parse_push_line_header(text: &str) -> Option<ParsedPushLineHeader> {
    let header = text
        .split_once(" · \"")
        .map(|(header, _)| header)
        .or_else(|| {
            text.rsplit_once(" · ")
                .filter(|(_, suffix)| suffix.starts_with("router://"))
                .map(|(header, _)| header)
        })
        .unwrap_or(text);
    let kind = if header.starts_with("✉️ ") || header.starts_with("🧑 Owner (unverified)") {
        PushKind::DirectMessage
    } else if header.starts_with("⏰ Router wake") {
        PushKind::Wake
    } else if header.starts_with("🗓 Router schedule") {
        PushKind::ScheduleRun
    } else if header.starts_with("❓ Router approval") {
        PushKind::Approval
    } else if header.starts_with("❓ Router question") {
        PushKind::Question
    } else if header.starts_with("🧵 Router: new thread activity") {
        PushKind::SubscriptionActivity
    } else if header.starts_with("🧵 Router: subscription expired") {
        PushKind::SubscriptionExpiry
    } else if header.starts_with("📋 Router: board notification") {
        PushKind::BoardNotification
    } else {
        return None;
    };
    Some(ParsedPushLineHeader {
        kind,
        title: header.to_owned(),
    })
}

#[cfg(test)]
#[path = "push_line_parse_tests.rs"]
mod tests;
