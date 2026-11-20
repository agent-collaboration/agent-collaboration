use super::parse_push_line;
use crate::{
    ParsedPushLineHeader, PushKind, RouterLink, escape_push_line_field, parse_push_line_header,
};

const SERVICE: &str = "018f47d2-24d5-7a68-b9ec-6f759c39458f";
const PUSH: &str = "018f47d2-24d5-7a68-b9ec-6f759c39458f";

#[test]
fn strict_push_line_accepts_real_short_and_clipped_deliveries() {
    let link = format!("router://{SERVICE}/push/{PUSH}");
    for line in [
        format!("🧑 Owner (unverified) @Sunbook → you · \"hello\" · {link}"),
        format!(
            "🧑 Owner (unverified) @Sunbook → you · \"long message …\" (+290) 📖 Load full message → {link}"
        ),
    ] {
        let parsed = parse_push_line(&line).expect("complete historical delivery");
        assert_eq!(parsed.kind, PushKind::DirectMessage);
        assert_eq!(parsed.link, RouterLink::parse(&link).unwrap());
    }
}

#[test]
fn strict_push_line_does_not_treat_plain_titles_as_deliveries() {
    for text in [
        "✉️ notes",
        "✉️ Main @Sunbook → you",
        "⏰ Router wake @Sunbook",
    ] {
        assert!(
            parse_push_line_header(text).is_some(),
            "display contract: {text}"
        );
        assert_eq!(parse_push_line(text), None, "delivery contract: {text}");
    }
}

#[test]
fn strict_push_line_rejects_incomplete_headers_and_malformed_links() {
    for link in [
        format!("router://not-a-uuid/push/{PUSH}"),
        format!("router://{SERVICE}/push/018f47d2-24d5-4a68-b9ec-6f759c39458f"),
        format!("router://{SERVICE}/push/{PUSH}/extra"),
        format!("router://{SERVICE}/push/{PUSH}?query"),
        format!("router://{SERVICE}/push/{PUSH}#fragment"),
        format!("router://{SERVICE}/push/{PUSH} trailing text"),
    ] {
        assert_eq!(
            parse_push_line(&format!("✉️ Main @Sunbook → you · {link}")),
            None
        );
    }
    for prefix in ["✉️ notes", "plain title", "", "✉️ Main @Sunbook → you\n"] {
        assert_eq!(
            parse_push_line(&format!("{prefix} · router://{SERVICE}/push/{PUSH}")),
            None
        );
    }
}

#[test]
fn strict_push_line_returns_every_header_kind() {
    for (header, expected) in [
        ("✉️ Main @Sunbook → you", PushKind::DirectMessage),
        ("⏰ Router wake @Sunbook", PushKind::Wake),
        (
            "🗓 Router schedule 018f47d2 @Sunbook · run 018f47d2",
            PushKind::ScheduleRun,
        ),
        (
            "❓ Router approval @Sunbook · Main asks",
            PushKind::Approval,
        ),
        (
            "❓ Router question @Sunbook · Main asks",
            PushKind::Question,
        ),
        (
            "🧵 Router: new thread activity @Sunbook · 1 thread · 2 messages",
            PushKind::SubscriptionActivity,
        ),
        (
            "🧵 Router: subscription expired @Sunbook · scope",
            PushKind::SubscriptionExpiry,
        ),
        (
            "📋 Router: board notification @Sunbook",
            PushKind::BoardNotification,
        ),
    ] {
        let parsed = parse_push_line(&format!("{header} · router://{SERVICE}/push/{PUSH}"))
            .expect("complete kind fixture");
        assert_eq!(parsed.kind, expected);
    }
}

#[test]
fn strict_push_line_rejects_an_expansion_prompt_without_a_rendered_preview() {
    let link = format!("router://{SERVICE}/push/{PUSH}");
    for content in [
        "⏰ Router wake @Sunbook",
        "⏰ Router wake @Sunbook · \"body\" (+0)",
        "⏰ Router wake @Sunbook · \"body\" (+not-a-count)",
        "⏰ Router wake @Sunbook · body (+3)",
    ] {
        assert_eq!(
            parse_push_line(&format!("{content} 📖 Load full message → {link}")),
            None
        );
    }
}

#[test]
fn header_parser_returns_display_facts_without_the_preview() {
    let line = format!(
        "✉️ 🤖 Main @machine → you · \"private preview text…\" (+1340) 📖 Load full message → router://{SERVICE}/push/{PUSH}"
    );
    let ParsedPushLineHeader { kind, title } =
        parse_push_line_header(&line).expect("display header");
    assert_eq!(kind, PushKind::DirectMessage);
    assert_eq!(title, "✉️ 🤖 Main @machine → you");
}

#[test]
fn machine_label_escapes_quotes_backslashes_controls_and_unicode_line_breaks() {
    let escaped = escape_push_line_field("Host\" \\ node\r\n\u{2028}\u{2029}\u{0001} · branch");
    assert_eq!(escaped, r#"Host\" \\ node⏎⏎⏎⏎\u{1} \u{b7} branch"#);
    assert_eq!(escaped.lines().count(), 1);
}

#[test]
fn router_link_parses_only_local_canonical_uuidv7_push_links() {
    let link = RouterLink::parse(&format!("router://{SERVICE}/push/{PUSH}")).unwrap();

    assert_eq!(
        RouterLink::parse(&link.to_string()).expect("valid link"),
        link
    );
    assert!(
        RouterLink::parse("https://machine/push/018f47d2-24d5-7a68-b9ec-6f759c39458f").is_err()
    );
    assert!(RouterLink::parse("router://018f47d2-24d5-7a68-b9ec-6f759c39458f/push/018f47d2-24d5-4a68-b9ec-6f759c39458f").is_err());
    assert!(RouterLink::parse("router://018f47d2-24d5-7a68-b9ec-6f759c39458f/push/018f47d2-24d5-7a68-b9ec-6f759c39458f/extra").is_err());
}
