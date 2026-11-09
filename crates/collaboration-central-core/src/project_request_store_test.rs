use super::*;
use crate::fleet_storage_fixture::{FleetStorageFixture, NOW, TestError};
use durable_steps::AtomicStore;
use sqlx_core::{query::query, row::Row};

#[expect(
    clippy::panic_in_result_fn,
    reason = "Assertions must fail this test while fixture errors propagate through Result."
)]
#[tokio::test]
async fn one_row_per_request_survives_routing_holder_delivery_and_independent_removal()
-> Result<(), TestError> {
    // Arrange: two requesters and a holder share the real Fleet store.
    let fixture = FleetStorageFixture::open().await?;
    let first = PeerId::new(durable_steps::Uuid::now_v7())?;
    let second = PeerId::new(durable_steps::Uuid::now_v7())?;
    let holder = PeerId::new(durable_steps::Uuid::now_v7())?;
    let build_request = |peer| -> Result<ProjectRequest, TestError> {
        Ok(ProjectRequest {
            forward_frame: FrameId::new(durable_steps::Uuid::now_v7())?,
            request_frame: FrameId::new(durable_steps::Uuid::now_v7())?,
            connection: ConnectionId::new(durable_steps::Uuid::now_v7())?,
            session: SessionKey {
                peer,
                session_id: fleet_protocol::SessionId::new(durable_steps::Uuid::now_v7())?,
            },
            project: "019f0000-0000-7000-8000-000000008199"
                .to_owned()
                .try_into()?,
            phase: ProjectRequestPhase::Routing,
            deadline: NOW.after(std::time::Duration::from_secs(30)),
            created_at: NOW,
            updated_at: NOW,
        })
    };
    let first_request = build_request(first)?;
    let second_request = build_request(second)?;
    let first_frame = first_request.forward_frame;
    let second_frame = second_request.forward_frame;
    let first_session = first_request.session;
    fixture
        .object
        .transaction(move |scope| {
            Box::pin(async move {
                // Act: all phases update the existing frame's row, retaining its envelope.
                for mut row in [first_request, second_request] {
                    for phase in [
                        ProjectRequestPhase::Routing,
                        ProjectRequestPhase::AwaitingHolder { holder },
                        ProjectRequestPhase::AtHolder { holder },
                    ] {
                        row.phase = phase;
                        save(scope, &row).await?;
                        let actual = by_frame(scope, row.forward_frame)
                            .await?
                            .ok_or_else(|| invalid("missing request"))?;
                        assert_eq!(actual.forward_frame, row.forward_frame);
                        assert_eq!(actual.request_frame, row.request_frame);
                        assert_eq!(actual.connection, row.connection);
                        assert_eq!(actual.session, row.session);
                        assert_eq!(actual.project, row.project);
                        assert_eq!(actual.deadline, row.deadline);
                        assert_eq!(actual.created_at, row.created_at);
                        assert_eq!(
                            serde_json::to_value(actual.phase).map_err(StoreError::backend)?,
                            serde_json::to_value(phase).map_err(StoreError::backend)?
                        );
                        let stored = query::<sqlx_turso::Turso>("SELECT COUNT(*) AS count FROM machine_project_requests WHERE request_frame_id=?")
                            .bind(row.request_frame.to_string()).fetch_one(scope.executor()).await.map_err(StoreError::backend)?;
                        assert_eq!(stored.try_get::<i64, _>("count").map_err(StoreError::backend)?, 1,
                            "routing, awaiting_holder and at_holder must each keep exactly one request row");
                    }
                }
                let stored = query::<sqlx_turso::Turso>(
                    "SELECT COUNT(*) AS count FROM machine_project_requests",
                )
                .fetch_one(scope.executor())
                .await
                .map_err(StoreError::backend)?;
                assert_eq!(
                    stored
                        .try_get::<i64, _>("count")
                        .map_err(StoreError::backend)?,
                    2,
                    "each request must occupy one row after delivery to the holder"
                );
                Ok(())
            })
        })
        .await?;
    let fixture = fixture.reopen().await?;
    fixture.object.transaction(move |scope| Box::pin(async move {
        // Assert: reopen keeps the same frame id and requester; completing A leaves B alone.
        assert_eq!(requests(scope).await?.len(), 2);
        let original = by_frame(scope, first_frame).await?.ok_or_else(|| invalid("missing request"))?;
        assert_eq!(original.session, first_session);
        assert!(matches!(original.phase, ProjectRequestPhase::AtHolder { holder: actual } if actual == holder));
        remove(scope, first_frame).await?;
        assert!(by_frame(scope, first_frame).await?.is_none());
        assert_eq!(by_frame(scope, second_frame).await?.map(|row| row.forward_frame), Some(second_frame));
        Ok(())
    })).await?;
    let fixture = fixture.reopen().await?;
    fixture
        .object
        .transaction(move |scope| {
            Box::pin(async move {
                assert_eq!(requests(scope).await?.len(), 1);
                assert!(by_frame(scope, first_frame).await?.is_none());
                Ok(())
            })
        })
        .await?;
    fixture.close().await?;
    Ok(())
}

#[expect(
    clippy::panic_in_result_fn,
    reason = "Assertions must fail this test while fixture errors propagate through Result."
)]
#[tokio::test]
async fn a_stored_unknown_request_phase_is_refused() -> Result<(), TestError> {
    let fixture = FleetStorageFixture::open().await?;
    let frame = FrameId::new(durable_steps::Uuid::now_v7())?;
    let origin = PeerId::new(durable_steps::Uuid::now_v7())?;
    let session = fleet_protocol::SessionId::new(durable_steps::Uuid::now_v7())?;
    let connection = ConnectionId::new(durable_steps::Uuid::now_v7())?;
    fixture.object.transaction(move |scope| Box::pin(async move {
        // Arrange: SQLite accepts the tag; the domain decoder must reject its meaning.
        query::<sqlx_turso::Turso>("INSERT INTO machine_project_requests(id,request_frame_id,requester_machine_id,requester_session_id,requester_connection_id,project_id,phase,deadline_at,created_at,updated_at) VALUES(?,?,?,?,?,?,'unrecognised',30,0,0)")
            .bind(frame.to_string()).bind(frame.to_string()).bind(origin.to_string()).bind(session.to_string()).bind(connection.to_string()).bind("019f0000-0000-7000-8000-000000008199")
            .execute(scope.executor()).await.map_err(StoreError::backend)?;
        assert!(matches!(by_frame(scope, frame).await, Err(StoreError::Corrupt { field, .. }) if field == "fleet.machine_project_requests.phase"));
        Ok(())
    })).await?;
    fixture.close().await?;
    Ok(())
}
