//! BC02: installed native authority, exact lanes and superseded non-post refusal.
use super::{
    central_pair::{SecondHost, await_view, await_writer, central, settled_view},
    fixture::{BoardFixture, accepted, at, identity},
    owned_host::BoardHost,
};
use codex_router_test_support::FixtureResult;
use serde_json::{Value, json};

pub async fn holder_change(fixture: &mut BoardFixture, second: &mut SecondHost) -> FixtureResult {
    fixture.create().await?;
    let before = settled_view(fixture.host()?, &fixture.project).await?;
    await_view(second.host()?, &fixture.project, at(&before, "/asOf")?).await?;
    let moved = central(fixture)?
        .move_project(
            fixture.project.clone().try_into()?,
            &second.service_id()?.try_into()?,
            identity().try_into()?,
        )
        .await?;
    eprintln!("BC02 owner move: {moved}");
    let installed = await_writer(second.host()?, &fixture.project, 2).await?;
    assert_eq!(installed.get("workstreams"), before.get("workstreams"));
    assert!(
        !at(&installed, "/workstreams")?
            .as_array()
            .ok_or("work lanes absent")?
            .is_empty()
    );
    assert_not_writer(fixture, fixture.host()?).await?;
    let moved_view = write_lane(fixture, second.host()?, fixture.host()?, 2).await?;
    let original = fixture
        .installation
        .as_ref()
        .ok_or("first identity absent")?
        .service_id
        .clone();
    let takeover = central(fixture)?
        .take_over_project(
            fixture.project.clone().try_into()?,
            &String::from(original).try_into()?,
            identity().try_into()?,
        )
        .await?;
    eprintln!("BC02 owner takeover: {takeover}");
    let restored = await_writer(fixture.host()?, &fixture.project, 3).await?;
    assert_eq!(restored.get("workstreams"), moved_view.get("workstreams"));
    assert_not_writer(fixture, second.host()?).await?;
    let final_view = write_lane(fixture, fixture.host()?, second.host()?, 3).await?;
    fixture.stop_host().await?;
    fixture.reopen_host().await?;
    assert_eq!(
        await_writer(fixture.host()?, &fixture.project, 3)
            .await?
            .get("workstreams"),
        final_view.get("workstreams")
    );
    Ok(())
}

pub async fn write_lane(
    fixture: &BoardFixture,
    writer: &BoardHost,
    reader: &BoardHost,
    epoch: u64,
) -> FixtureResult<Value> {
    let lane = identity();
    let marker = format!("actual epoch {epoch} lane {lane}");
    let request = fixture.request(
        &fixture.owner,
        json!({"kind":"workstreamCreate",
        "workstreamId":lane,"name":marker,"goal":"observe actual installed epoch admission"}),
    );
    accepted(&writer.mcp("workstream_create", request).await?)?;
    let view = settled_view(writer, &fixture.project).await?;
    assert_eq!(view.pointer("/asOf/epoch"), Some(&json!(epoch)));
    let copied = await_view(reader, &fixture.project, at(&view, "/asOf")?).await?;
    assert_eq!(copied.get("workstreams"), view.get("workstreams"));
    let lanes = at(&copied, "/workstreams")?
        .as_array()
        .ok_or("copied lanes absent")?;
    assert!(
        lanes
            .iter()
            .any(|row| row.get("id") == Some(&json!(lane))
                && row.get("name") == Some(&json!(marker)))
    );
    Ok(view)
}

async fn assert_not_writer(
    fixture: &BoardFixture,
    host: &super::owned_host::BoardHost,
) -> FixtureResult {
    let request = fixture.request(
        &fixture.owner,
        json!({"kind":"projectDescribe",
        "description":"forbidden superseded mutation","expectedRevision":0}),
    );
    let reply = host.mcp("project_describe", request).await?;
    assert_eq!(reply.pointer("/result/kind"), Some(&json!("notWriter")));
    Ok(())
}

pub async fn directory_expiry(
    fixture: &mut BoardFixture,
    second: &mut SecondHost,
) -> FixtureResult {
    use board_central_test_support::{ObjectAddress, RuntimeControl};
    fixture.create().await?;
    let before = settled_view(fixture.host()?, &fixture.project).await?;
    let copied = await_view(second.host()?, &fixture.project, at(&before, "/asOf")?).await?;
    let directory = central(fixture)?
        .directory_entry(&fixture.project.clone().try_into()?)
        .await?;
    let expires = chrono::DateTime::parse_from_rfc3339(
        at(&directory, "/leaseSide/value/lease/value/expires_at")?
            .as_str()
            .ok_or("committed expiry absent")?,
    )?;
    let boundary = expires.with_timezone(&chrono::Utc);
    central(fixture)?
        .runtime_control(
            ObjectAddress::Fleet,
            RuntimeControl::SetTime {
                now: serde_json::from_value(json!(boundary - chrono::TimeDelta::milliseconds(1)))?,
            },
        )
        .await?;
    let held = central(fixture)?
        .directory_entry(&fixture.project.clone().try_into()?)
        .await?;
    let held_view = super::central_pair::work_view(second.host()?, &fixture.project).await?;
    assert_eq!(
        held.pointer("/leaseSide/value/lease/value/state"),
        Some(&json!("held"))
    );
    central(fixture)?
        .runtime_control(
            ObjectAddress::Fleet,
            RuntimeControl::SetTime {
                now: serde_json::from_value(json!(boundary))?,
            },
        )
        .await?;
    let expired = central(fixture)?
        .directory_entry(&fixture.project.clone().try_into()?)
        .await?;
    let expired_view = super::central_pair::work_view(second.host()?, &fixture.project).await?;
    assert_eq!(
        expired.pointer("/leaseSide/value/lease/value/state"),
        Some(&json!("expired"))
    );
    for pointer in ["/leaseSide/value/leaseVersion", "/leaseSide/value/asOf"] {
        assert_eq!(at(&held, pointer)?, at(&expired, pointer)?);
        assert_eq!(at(&directory, pointer)?, at(&expired, pointer)?);
    }
    assert_eq!(
        expired_view.get("writerObservation"),
        Some(&json!({
        "leaseVersion":at(&expired,"/leaseSide/value/leaseVersion")?,
        "asOf":at(&expired,"/leaseSide/value/asOf")?}))
    );
    assert_eq!(held_view, expired_view);
    assert_eq!(copied.get("workstreams"), expired_view.get("workstreams"));
    Ok(())
}
