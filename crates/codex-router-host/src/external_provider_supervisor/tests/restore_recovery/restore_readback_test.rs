use super::*;
use collaboration_protocol::ProviderSessionInspectRequest;

#[tokio::test]
async fn a_ready_host_restore_publishes_the_setters_catalogue_and_keeps_saved_setup() {
    let rig = RestoreRig::new();
    rig.seed_session().await;
    let mut store = rig.store().await;
    let mut record = store
        .session_record(&rig.target)
        .await
        .expect("read session setup")
        .expect("seeded session");
    record.resume.requested_mode = Some("plan".to_owned());
    store
        .record_session(&record)
        .await
        .expect("seed saved Plan mode");
    drop(store);
    let host = rig.open_host("restored-in-auto").await;

    rig.assert_loads(&host).await;
    assert_eq!(
        rig.wire_receipt()["modeRequests"],
        serde_json::json!([{"sessionId":rig.target.session_id,"value":"plan"}]),
    );
    let shown = host
        .inspect_session(ProviderSessionInspectRequest {
            target: rig.target.clone(),
        })
        .await
        .expect("read active catalogue");
    assert_eq!(
        shown
            .settings_catalog
            .expect("ready session settings")
            .current_mode
            .as_deref(),
        Some("plan"),
    );
    let owner = host
        .session_owner_for(&rig.target)
        .expect("ready session owner");
    assert_eq!(
        owner
            .catalog()
            .expect("active ACP catalogue")
            .effective_settings()
            .mode
            .as_deref(),
        Some("plan"),
        "the ready Host routes subsequent work with the setter's catalogue",
    );
    let saved = rig
        .store()
        .await
        .session_record(&rig.target)
        .await
        .expect("read stored setup after load")
        .expect("saved session remains");
    assert_eq!(saved.resume, record.resume);
    host.shutdown().await.expect("fixture shutdown");
}
