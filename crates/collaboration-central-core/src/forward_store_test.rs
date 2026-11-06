use super::*;
use crate::fleet_storage_fixture::{FleetStorageFixture, NOW, TestError};
use durable_steps::AtomicStore;
use sqlx_core::query::query;

#[expect(
    clippy::panic_in_result_fn,
    reason = "Assertions must fail this test while fixture errors propagate through Result."
)]
#[tokio::test]
async fn origin_and_holder_correlations_coexist_for_two_origins_and_delete_independently()
-> Result<(), TestError> {
    // Arrange: three real enrollment rows, two independent origin sessions and one holder.
    let fixture = FleetStorageFixture::open().await?;
    let first = PeerId::new(durable_steps::Uuid::now_v7())?;
    let second = PeerId::new(durable_steps::Uuid::now_v7())?;
    let holder = PeerId::new(durable_steps::Uuid::now_v7())?;
    let holder_service = "019f0000-0000-7000-8000-000000008103";
    fixture
        .enroll(first, "019f0000-0000-7000-8000-000000008101")
        .await?;
    fixture
        .enroll(second, "019f0000-0000-7000-8000-000000008102")
        .await?;
    fixture.enroll(holder, holder_service).await?;
    let build_pending_forward = |peer| -> Result<PendingForward, TestError> {
        Ok(PendingForward {
            forward_frame: FrameId::new(durable_steps::Uuid::now_v7())?,
            request_frame: FrameId::new(durable_steps::Uuid::now_v7())?,
            connection: ConnectionId::new(durable_steps::Uuid::now_v7())?,
            session: SessionKey {
                peer,
                session_id: fleet_sync_protocol::SessionId::new(durable_steps::Uuid::now_v7())?,
            },
            project: "019f0000-0000-7000-8000-000000008199"
                .to_owned()
                .try_into()?,
            phase: ForwardPhase::AwaitingHolder {
                installation: holder_service.to_owned().try_into()?,
            },
            deadline: NOW.after(std::time::Duration::from_secs(30)),
            created_at: NOW,
            updated_at: NOW,
        })
    };
    let first_pending = build_pending_forward(first)?;
    let second_pending = build_pending_forward(second)?;
    let first_frame = first_pending.forward_frame;
    let second_frame = second_pending.forward_frame;
    let first_session = first_pending.session;
    fixture
        .object
        .transaction(move |scope| {
            Box::pin(async move {
                // Act: each frame has an origin side and a holder side in one physical store.
                for (row, sender) in [
                    (&first_pending, "019f0000-0000-7000-8000-000000008101"),
                    (&second_pending, "019f0000-0000-7000-8000-000000008102"),
                ] {
                    save_pending(scope, row).await?;
                    save_delivered(
                        scope,
                        &DeliveredForward {
                            forward_frame: row.forward_frame,
                            request_frame: row.request_frame,
                            connection: row.connection,
                            origin_session: row.session,
                            sender: sender.to_owned().try_into().map_err(StoreError::backend)?,
                            project: row.project.clone(),
                            holder,
                            deadline: row.deadline,
                            created_at: NOW,
                            updated_at: NOW,
                        },
                    )
                    .await?;
                }
                Ok(())
            })
        })
        .await?;
    let fixture = fixture.reopen().await?;
    let (origin_count, holder_count, original_session) = fixture
        .object
        .transaction(move |scope| {
            Box::pin(async move {
                let origins = pending(scope).await?;
                let holders = delivered(scope).await?;
                let original_session = delivered_by_frame(scope, first_frame)
                    .await?
                    .ok_or_else(|| invalid("missing holder side"))?
                    .origin_session;
                remove_delivered(scope, first_frame).await?;
                Ok((origins.len(), holders.len(), original_session))
            })
        })
        .await?;
    let fixture = fixture.reopen().await?;
    let (surviving_origin, surviving_other, removed_holder) = fixture
        .object
        .transaction(move |scope| {
            Box::pin(async move {
                Ok((
                    pending_by_frame(scope, first_frame)
                        .await?
                        .map(|row| row.forward_frame),
                    delivered_by_frame(scope, second_frame)
                        .await?
                        .map(|row| row.forward_frame),
                    delivered_by_frame(scope, first_frame)
                        .await?
                        .map(|row| row.forward_frame),
                ))
            })
        })
        .await?;
    assert_eq!(removed_holder, None);
    // Assert: sharing an id never collapses the two responsibilities or the two origins.
    assert_eq!((origin_count, holder_count), (2, 2));
    assert_eq!(original_session, first_session);
    assert_eq!(surviving_origin, Some(first_frame));
    assert_eq!(surviving_other, Some(second_frame));
    assert!(fixture.root.path().join("fleet.db").is_file());
    fixture.close().await?;
    Ok(())
}

#[expect(
    clippy::panic_in_result_fn,
    reason = "Assertions must fail this test while fixture errors propagate through Result."
)]
#[tokio::test]
async fn a_stored_unknown_forward_phase_is_refused() -> Result<(), TestError> {
    let fixture = FleetStorageFixture::open().await?;
    let frame = FrameId::new(durable_steps::Uuid::now_v7())?;
    let origin = PeerId::new(durable_steps::Uuid::now_v7())?;
    let session = fleet_sync_protocol::SessionId::new(durable_steps::Uuid::now_v7())?;
    let connection = ConnectionId::new(durable_steps::Uuid::now_v7())?;
    fixture.object.transaction(move |scope| Box::pin(async move {
        // Arrange: storage affinity accepts the text, so Rust must reject its meaning.
        query::<sqlx_turso::Turso>("INSERT INTO machine_forward_requests(id,forward_frame_id,side,request_frame_id,origin_machine_id,origin_session_id,connection_id,project_id,phase,deadline_at,created_at,updated_at) VALUES(?,?,'origin',?,?,?,?,?,'unrecognised',30,0,0)")
            .bind(durable_steps::Uuid::now_v7().to_string()).bind(frame.to_string()).bind(frame.to_string()).bind(origin.to_string()).bind(session.to_string()).bind(connection.to_string()).bind("019f0000-0000-7000-8000-000000008199")
            .execute(scope.executor()).await.map_err(StoreError::backend)?;
        // Act / Assert: the real query and decoder refuse the invalid stored state.
        assert!(matches!(pending_by_frame(scope, frame).await, Err(StoreError::Corrupt { field, .. }) if field == "fleet.machine_forward_requests.phase"));
        Ok(())
    })).await?;
    fixture.close().await?;
    Ok(())
}
