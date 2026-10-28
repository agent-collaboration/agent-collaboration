use super::super::{
    HistoryArguments, InboxArguments, MessageArguments, MessageCommand, ReplyArguments,
};
use super::reply_confirmation_line;
use clap::Parser;

#[test]
fn reply_requires_a_reference_and_keeps_text_file_input() {
    const PUSH_ID: &str = "019f0000-0000-7000-8000-000000000101";

    let text_request = MessageArguments::try_parse_from([
        "agent-collaboration message",
        "reply",
        PUSH_ID,
        "hello",
    ])
    .expect("positional reply arguments parse");
    assert!(matches!(
        text_request.command,
        MessageCommand::Reply(ReplyArguments {
            reference,
            text: Some(ref text),
            text_file: None,
            ..
        }) if reference == PUSH_ID && text == "hello"
    ));

    let file_request = MessageArguments::try_parse_from([
        "agent-collaboration message",
        "reply",
        PUSH_ID,
        "--text-file",
        "reply.md",
    ])
    .expect("file reply arguments parse");
    assert!(matches!(
        file_request.command,
        MessageCommand::Reply(ReplyArguments {
            reference,
            text: None,
            text_file: Some(_),
            ..
        }) if reference == PUSH_ID
    ));

    assert!(
        MessageArguments::try_parse_from([
            "agent-collaboration message",
            "reply",
            "--text",
            "hello",
        ])
        .is_err()
    );

    assert!(
        MessageArguments::try_parse_from([
            "agent-collaboration message",
            "reply",
            PUSH_ID,
            "hello",
            "--expect-sender",
            "{\"endpoint\":{},\"sessionId\":\"sender\"}",
        ])
        .is_err()
    );
}

#[test]
fn inbox_and_history_are_message_subcommands() {
    assert!(matches!(
        MessageArguments::try_parse_from(["agent-collaboration message", "inbox"])
            .expect("inbox arguments parse")
            .command,
        MessageCommand::Inbox(InboxArguments { .. })
    ));
    assert!(matches!(
        MessageArguments::try_parse_from([
            "agent-collaboration message",
            "history",
            "--with",
            "{\"endpoint\":{},\"sessionId\":\"other\"}",
        ])
        .expect("history arguments parse")
        .command,
        MessageCommand::History(HistoryArguments { .. })
    ));
}

#[test]
fn reply_confirmation_names_the_recipient_push_and_delivery_outcome() {
    const PUSH_ID: &str = "019f0000-0000-7000-8000-000000000101";
    const PUSH_LINK: &str =
        "router://00000000-0000-4000-8000-000000000001/push/019f0000-0000-7000-8000-000000000101";
    let target: collaboration_client::protocol::SessionRef = serde_json::from_value(
        serde_json::json!({
            "endpoint":{"serviceId":"00000000-0000-4000-8000-000000000001","endpointId":"claude-local"},
            "sessionId":"reply-target"
        }),
    )
    .expect("reply target");
    let reply = collaboration_client::protocol::SessionMessageReplyResult {
        target: target.clone(),
        target_identity: "✳️ Claude Main".to_owned(),
        push_id: PUSH_ID.to_owned().try_into().expect("push id"),
        link: PUSH_LINK.to_owned(),
        delivery_state: collaboration_client::protocol::PushDeliveryState::Delivered,
        receipt: collaboration_client::protocol::DeliveryReceipt {
            outcome: collaboration_client::protocol::DeliveryOutcome::PeerMessageWritten,
            reachability: Some(collaboration_client::protocol::SessionReachability::ClaudeCodePeer),
            client: Some(collaboration_client::protocol::DeliveryClientReceipt::ClaudeCodePeer),
        },
    };

    assert_eq!(
        reply_confirmation_line(&reply),
        format!(
            "replied to ✳️ Claude Main {} (push {PUSH_ID} {PUSH_LINK}): peer message written",
            serde_json::to_string(&target).expect("target JSON"),
        ),
    );
}
