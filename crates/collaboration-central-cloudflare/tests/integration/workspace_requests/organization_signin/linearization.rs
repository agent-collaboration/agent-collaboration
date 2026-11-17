use super::support::*;
use collaboration_central_conformance::signin_fixtures::{self as stamp, NOW_SECONDS};
use serde_json::json;
#[tokio::test]
async fn expiry_crossed_before_sample_refuses_and_post_sample_time_remains_linearized() {
    let f = Fixture::new().await;
    let runtime = f.runtime().await;
    let mut claims = stamp::claims(ORG, NOW_SECONDS);
    claims["iat"] = json!(NOW_SECONDS - 60);
    claims["exp"] = json!(NOW_SECONDS + 1);
    let compact = stamp::sign(&stamp::stamp_header(), &claims, 7).unwrap();
    let body = json!({"organizationId":ORG,"organizationStatement":compact});
    let _: bool = runtime
        .control("hold-central", &"beforeSignInSample")
        .await
        .unwrap();
    let origin = f.host.signin_origin();
    let first_body = body.clone();
    let first = tokio::spawn(async move {
        reqwest::Client::new()
            .post(format!("{origin}{PROVISION}"))
            .json(&first_body)
            .send()
            .await
            .unwrap()
            .status()
            .as_u16()
    });
    let _: bool = runtime
        .control("until-held-central", &"beforeSignInSample")
        .await
        .unwrap();
    f.clock
        .set_utc(chrono::DateTime::from_timestamp(NOW_SECONDS + 1, 0).unwrap());
    f.runtime().await;
    let _: bool = runtime
        .control("release-central", &"beforeSignInSample")
        .await
        .unwrap();
    assert_eq!(first.await.unwrap(), 422);
    assert_eq!(
        f.read("SELECT id FROM organization_identity")
            .await
            .rows
            .len(),
        0
    );
    f.clock
        .set_utc(chrono::DateTime::from_timestamp(NOW_SECONDS, 0).unwrap());
    f.runtime().await;
    let _: bool = runtime
        .control("hold-central", &"afterSignInSample")
        .await
        .unwrap();
    let origin = f.host.signin_origin();
    let first_body = body;
    let first = tokio::spawn(async move {
        reqwest::Client::new()
            .post(format!("{origin}{PROVISION}"))
            .json(&first_body)
            .send()
            .await
            .unwrap()
            .status()
            .as_u16()
    });
    let _: bool = runtime
        .control("until-held-central", &"afterSignInSample")
        .await
        .unwrap();
    f.clock
        .set_utc(chrono::DateTime::from_timestamp(NOW_SECONDS + 1, 0).unwrap());
    f.runtime().await;
    let origin = f.host.signin_origin();
    let second = tokio::spawn(async move {
        reqwest::Client::new()
            .post(format!("{origin}{REVISIONS}"))
            .json(&org_revision(1, "active"))
            .send()
            .await
            .unwrap()
            .status()
            .as_u16()
    });
    let _: bool = runtime
        .control("until-central-waiting", &1usize)
        .await
        .unwrap();
    let _: bool = runtime
        .control("release-central", &"afterSignInSample")
        .await
        .unwrap();
    assert_eq!(first.await.unwrap(), 200);
    assert_eq!(second.await.unwrap(), 503);
    assert!(
        f.read("SELECT id FROM organization_authorization_floors")
            .await
            .rows
            .is_empty(),
        "the queued writer observes expiry and cannot store a floor"
    );
    assert_eq!(
        f.read("SELECT id FROM organization_change_records")
            .await
            .rows
            .len(),
        1,
        "only the valid earlier provisioning sample commits its record"
    );
    let rows = f.read("SELECT created_at FROM organization_identity").await;
    assert_eq!(
        rows.rows,
        vec![vec![central_storage::SqlValue::Integer(
            (NOW_SECONDS * 1000).try_into().unwrap()
        )]]
    );
    f.close().await;
}
#[tokio::test]
async fn provisioning_first_stamp_arms_pending_wake_and_actual_alarm_fires_once() {
    use central_storage::SqlValue;
    let f = Fixture::new().await;
    let runtime = f.runtime().await;
    let due = chrono::DateTime::from_timestamp((NOW_SECONDS.div_euclid(300) + 1) * 300, 0).unwrap();
    let target = workspace_model::SessionAddress {
        machine_id: "019f0000-0000-7000-8000-00000000d222".parse().unwrap(),
        endpoint: "codex-local".to_owned().try_into().unwrap(),
        session_id: "pending-signin".to_owned().try_into().unwrap(),
    };
    // Arrange pending durable work before the first organization authority exists.
    // Authenticated SetWake cannot run before provisioning at this hard cut.
    f.execute("INSERT INTO machines(machine_id,installation_id,label,state,allowed_workspaces,created_at,updated_at) VALUES (?,?,'Pending wake host','enrolled','[]',?,?)",vec![SqlValue::Text("019f0000-0000-7000-8000-00000000d224".to_owned()),SqlValue::Text(target.machine_id.to_string()),SqlValue::Text(due.to_rfc3339()),SqlValue::Text(due.to_rfc3339())]).await;
    // As SetWake would store it: canonical millisecond UTC, created now, due later.
    let stored_due = due.to_rfc3339_opts(chrono::SecondsFormat::Millis, true);
    let created = chrono::DateTime::from_timestamp(NOW_SECONDS, 0)
        .unwrap()
        .to_rfc3339_opts(chrono::SecondsFormat::Millis, true);
    f.execute("INSERT INTO wakes(wake_id,target,due_at,text,state,created_at,updated_at) VALUES (?,?,?,?,'active',?,?)",vec![SqlValue::Text("019f0000-0000-7000-8000-00000000d226".to_owned()),SqlValue::Text(serde_json::to_string(&target).unwrap()),SqlValue::Text(stored_due.clone()),SqlValue::Text(serde_json::to_string("pending").unwrap()),SqlValue::Text(created.clone()),SqlValue::Text(created)]).await;
    let before: Option<i64> = runtime.control("raw-alarm-time", &()).await.unwrap();
    assert_eq!(before, None);
    assert_eq!(f.provision().await.0, 200);
    let armed: Option<i64> = runtime.control("raw-alarm-time", &()).await.unwrap();
    assert_eq!(armed, Some(due.timestamp_millis()));
    f.clock
        .advance(
            (due - chrono::DateTime::from_timestamp(NOW_SECONDS, 0).unwrap())
                .to_std()
                .unwrap(),
        )
        .unwrap();
    assert_eq!(runtime.after_event().await.unwrap().wakes_fired, 1);
    assert_eq!(runtime.after_event().await.unwrap().wakes_fired, 0);
    let rows=f.read("SELECT (SELECT COUNT(*) FROM wakes WHERE state='done'),(SELECT COUNT(*) FROM messages WHERE message_kind='wake')").await;
    assert_eq!(
        rows.rows,
        vec![vec![
            SqlValue::Integer(1.try_into().unwrap()),
            SqlValue::Integer(1.try_into().unwrap())
        ]]
    );
    f.close().await;
}
