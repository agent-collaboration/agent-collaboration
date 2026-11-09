use super::*;

#[tokio::test]
async fn missing_mail_owner_preserves_the_available_central_link_record() {
    let record = board_sync_protocol::FleetLinkRecord {
        peer: fleet_sync_protocol::PeerId::new(uuid::Uuid::now_v7()).unwrap(),
        endpoint: "wss://central.example/session".to_owned(),

        instance: 0,
        connection: 0,
        revision: 0,
        last_frame_at: None,
        state: board_sync_protocol::FleetLinkState::Disconnected {
            since: chrono::Utc::now(),
        },
        last_drop: None,
        last_failure: None,
    };
    let source = FleetLinkObservation::Failed(Box::new(record.clone()));
    let observed = observe_fleet_sources(&source, &Weak::new()).await;
    let crate::FleetHostObservation::Available {
        central: Some(central),
        stuck_mail,
    } = observed
    else {
        panic!("mail read failure hid the available central observation");
    };
    assert_eq!(*central, record);
    assert!(matches!(stuck_mail, StuckMailSummary::Unavailable { .. }));
}
