use super::*;

#[test]
fn fleet_origin_round_trips_and_renders_a_fleet_sender() {
    let origin: PushOrigin = serde_json::from_str("\"fleet\"").unwrap();
    let id = "019f0000-0000-7000-8000-000000000001";
    let line = render_push_line(&PushLineInput {
        link: RouterLink::new(
            id.to_owned().try_into().unwrap(),
            id.to_owned().try_into().unwrap(),
        ),
        machine_label: "A".to_owned().try_into().unwrap(),
        origin: origin.clone(),
        header_facts: PushHeaderFacts::DirectMessage {
            sender_display_name: None,
        },
        body: Some("remote create completed".into()),
    })
    .unwrap();
    assert_eq!(serde_json::to_string(&origin).unwrap(), "\"fleet\"");
    assert!(line.contains("Fleet @A"), "{line}");
    assert_eq!(
        parse_push_line_header(&line).unwrap().kind,
        PushKind::DirectMessage
    );
}
