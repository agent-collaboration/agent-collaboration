use collaboration_protocol::*;
use message_board::{NotifyIntentId, ProjectId, RoleAssignmentId};
use serde_json::json;

#[allow(clippy::expect_used)] // This test-only helper asserts a typed static fixture.
fn draft(summary: &str) -> PushRecordDraft {
    let project = ProjectId::generate();
    let intent = NotifyIntentId::generate();
    let origin = format!(
        "{{\"kind\":\"boardIntent\",\"project\":\"{}\",\"intent\":\"{}\"}}",
        project.as_str(),
        intent.as_str()
    );
    serde_json::from_value(json!({
        "pushId":uuid::Uuid::now_v7().to_string(), "kind":"board-notification",
        "origin":{"router":"board-notification"}, "originRouterRef":origin,
        "target":{"endpoint":{"serviceId":"018f47d2-24d5-7a68-b9ec-6f759c39458f","endpointId":"codex-local"},"sessionId":"recipient"},
        "replyToPushId":null,
        "headerFacts":{"kind":"boardNotification","project":project,"intent":intent,"cause":{"kind":"roleAssignment","id":RoleAssignmentId::generate()},"summary":summary},
        "body":null,"activity":null,"createdAt":chrono::Utc::now()
    })).expect("typed board notification draft")
}

#[test]
fn role_intent_without_a_thread_uses_the_real_renderer_and_canonical_source_link() {
    let record = draft("Lead assigned\nread the project")
        .into_pending()
        .unwrap();
    let origin =
        RouterOriginRef::parse_canonical(record.origin_router_ref.as_deref().unwrap()).unwrap();
    assert!(origin.supports_kind(record.kind));
    let link = RouterLink::new(
        MachineId::try_from("018f47d2-24d5-7a68-b9ec-6f759c39458f".to_owned()).unwrap(),
        record.push_id.clone(),
    );
    let line = render_push_line(&PushLineInput {
        link: link.clone(),
        machine_label: MachineLabel::try_from("test-machine".to_owned()).unwrap(),
        origin: record.origin,
        header_facts: record.header_facts,
        body: None,
    })
    .unwrap();
    assert!(line.starts_with("📋 Router: board notification"));
    assert!(line.contains("Lead assigned⏎read the project"));
    assert!(line.ends_with(&link.to_string()));
    assert_eq!(
        parse_push_line_header(&line).unwrap().kind,
        serde_json::from_value::<PushKind>(json!("board-notification")).unwrap()
    );
    assert!(!line.contains('\n'));
}

#[test]
fn long_board_summary_is_rendered_within_the_existing_line_budget() {
    let record = draft(&"😀\"·".repeat(1000)).into_pending().unwrap();
    let line = render_push_line(&PushLineInput {
        link: RouterLink::new(
            MachineId::try_from("018f47d2-24d5-7a68-b9ec-6f759c39458f".to_owned()).unwrap(),
            record.push_id,
        ),
        machine_label: MachineLabel::try_from("machine".to_owned()).unwrap(),
        origin: record.origin,
        header_facts: record.header_facts,
        body: None,
    })
    .unwrap();
    assert!(line.len() <= MAX_PUSH_LINE_BYTES);
    assert!(line.contains("(+"));
}

#[test]
fn a_board_push_cannot_point_to_another_intent_or_fabricate_thread_ranges() {
    let mut mismatched = draft("Lead assigned");
    mismatched.origin_router_ref = Some(format!(
        "{{\"kind\":\"boardIntent\",\"project\":\"{}\",\"intent\":\"{}\"}}",
        ProjectId::generate().as_str(),
        NotifyIntentId::generate().as_str()
    ));
    assert!(mismatched.into_pending().is_err());
    let mut invalid = draft("Lead assigned");
    invalid.activity = Some(PushActivitySnapshot {
        ranges: Vec::new(),
        held: false,
        draining: false,
    });
    assert!(invalid.into_pending().is_err());
}
