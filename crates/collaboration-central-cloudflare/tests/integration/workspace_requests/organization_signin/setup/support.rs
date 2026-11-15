//! Shared setup proof inputs and owning-row oracles, with real signed requests.
pub(super) use super::super::support::{Fixture, LOOKUP, ORG, REVISIONS, person_revision};
pub(super) use collaboration_central_conformance::signin_fixtures::{self as signed, NOW_SECONDS};
pub(super) use serde_json::{Value, json};
pub(super) fn number(value: i64) -> central_storage::SqlValue {
    central_storage::SqlValue::Integer(value.try_into().unwrap())
}
pub(super) fn operation(serial: u16) -> String {
    format!("019f0000-0000-7000-8000-00000000d{serial:03x}")
}
pub(super) fn create(operation: &str, name: &str, description: &str) -> (Value, Value) {
    (
        json!({"organizationId":ORG,"operationId":operation,"name":name,"description":description}),
        json!({"v":1,"operation":"CreateWorkspace","organization":ORG,"target":ORG,"operationId":operation,"params":{"name":name,"description":description}}),
    )
}
pub(super) fn headers(
    person: &signed::Person,
    intent: &Value,
    now: i64,
) -> Vec<(&'static str, String)> {
    person
        .headers(
            signed::CREATE,
            intent,
            &signed::fresh_request_id(now).unwrap(),
            now,
        )
        .unwrap()
}
pub(super) async fn ready() -> Fixture {
    let f = Fixture::new().await;
    assert_eq!(f.provision().await.0, 200);
    f.floors().await;
    f
}
pub(super) async fn member_floor(f: &Fixture) {
    assert_eq!(f.post(REVISIONS,&json!({"kind":"person","organizationId":ORG,"personId":signed::MEMBER,"subjectId":signed::MEMBER_SUBJECT,"revision":1,"state":"active"})).await.0,200);
}
pub(super) async fn submit(
    f: &Fixture,
    person: &signed::Person,
    operation: &str,
    name: &str,
) -> (u16, Value) {
    let (body, intent) = create(operation, name, "");
    f.post_with(
        signed::CREATE,
        &body,
        &headers(person, &intent, NOW_SECONDS),
    )
    .await
}
pub(super) async fn stable_effects(f: &Fixture) -> Vec<Vec<Vec<central_storage::SqlValue>>> {
    let mut rows = Vec::new();
    for sql in [
        "SELECT * FROM workspaces ORDER BY workspace_id",
        "SELECT * FROM request_log ORDER BY request_id",
        "SELECT * FROM organization_change_records ORDER BY id",
        "SELECT * FROM actor_identities ORDER BY actor_identity_id",
    ] {
        rows.push(f.read(sql).await.rows);
    }
    rows
}
pub(super) async fn crash(f: &Fixture) {
    let reset: Result<bool, central_storage::StoreFailure> =
        f.runtime().await.control("crash", &()).await;
    assert!(reset.is_err());
    f.runtime().await;
}
