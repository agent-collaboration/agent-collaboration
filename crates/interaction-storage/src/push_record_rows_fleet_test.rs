use crate::InteractionStore;
use collaboration_protocol::{PushOrigin, PushRecordDraft};
use serde_json::json;

#[tokio::test]
async fn fleet_push_origin_survives_the_existing_store_and_reopen() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("interaction.sqlite");
    let store = InteractionStore::open(path.clone()).await.unwrap();
    let id = "019f0000-0000-7000-8000-000000000001";
    let draft: PushRecordDraft = serde_json::from_value(json!({"pushId":id,"kind":"direct-message",
        "origin":"fleet","originRouterRef":null,
        "target":{"endpoint":{"serviceId":id,"endpointId":"codex-local"},"sessionId":"requester"},
        "mode":"auto","guard":null,"replyToPushId":null,
        "headerFacts":{"kind":"directMessage","senderDisplayName":null},"body":"remote create result",
        "activity":null,"createdAt":"2026-11-05T12:00:00Z"})).unwrap();
    let inserted = store.insert_push_record(draft).await.unwrap();
    store.close().await.unwrap();
    let reopened = InteractionStore::open(path).await.unwrap();
    let stored = reopened
        .get_push_record(&inserted.push_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(stored.origin, PushOrigin::Fleet);
    assert_eq!(stored.body, inserted.body);
    reopened.close().await.unwrap();
}
