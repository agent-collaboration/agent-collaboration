use super::parse_push_line;
use crate::{
    MachineId, MachineLabel, PushHeaderFacts, PushId, PushKind, PushLineInput, PushOrigin,
    RouterLink, parse_push_line_header, render_push_line,
};

const SERVICE: &str = "018f47d2-24d5-7a68-b9ec-6f759c39458f";
const PUSH: &str = "018f47d2-24d5-7a68-b9ec-6f759c39458f";

#[test]
fn strict_push_line_accepts_real_short_and_clipped_deliveries() {
    for body in ["hello".to_owned(), "long message ".repeat(30)] {
        let input = PushLineInput {
            link: RouterLink::new(
                MachineId::try_from(SERVICE.to_owned()).unwrap(),
                PushId::try_from(PUSH.to_owned()).unwrap(),
            ),
            machine_label: MachineLabel::try_from("Sunbook".to_owned()).unwrap(),
            origin: PushOrigin::OwnerUnverified,
            header_facts: PushHeaderFacts::DirectMessage {
                sender_display_name: None,
            },
            body: Some(body),
        };
        let rendered = render_push_line(&input).unwrap();
        let parsed = parse_push_line(&rendered).expect("complete rendered delivery");
        assert_eq!(parsed.kind, PushKind::DirectMessage);
        assert_eq!(parsed.link, input.link);
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
