use super::{
    DurableSnapshot, RegistrationRow, RegistrationState, SessionRecord, WindowRecord, compose,
};
use durable_steps::Uuid;
use fleet_protocol::{ConnectionId, PeerId, SchemaSupport, SessionId};
use fleet_session::ConnectionRegistration;

type TestError = Box<dyn std::error::Error + Send + Sync>;

fn attached(session: SessionId) -> ConnectionRegistration {
    ConnectionRegistration::Attached {
        session_id: session,
        schema_support: SchemaSupport { max_version: 7 },
        capabilities: None,
    }
}

#[expect(
    clippy::panic_in_result_fn,
    reason = "Assertions must fail this test while fixture errors propagate through Result."
)]
#[test]
fn every_registration_is_reported_and_windows_count_only_the_attached_session()
-> Result<(), TestError> {
    // Arrange: an attached connection with rows on its current session and on
    // an older one, an opening connection, and a closed connection that still
    // has rows (stale, sent before it closed).
    let peer = PeerId::new(Uuid::now_v7())?;
    let live = ConnectionId::new(Uuid::now_v7())?;
    let opening = ConnectionId::new(Uuid::now_v7())?;
    let closed = ConnectionId::new(Uuid::now_v7())?;
    let current = SessionId::new(Uuid::now_v7())?;
    let older = Uuid::now_v7();
    let registrations = vec![
        RegistrationRow {
            connection: live,
            peer,
            registration: attached(current),
        },
        RegistrationRow {
            connection: opening,
            peer,
            registration: ConnectionRegistration::Opening,
        },
        RegistrationRow {
            connection: closed,
            peer,
            registration: ConnectionRegistration::Closed,
        },
    ];
    let durable = DurableSnapshot {
        in_flight: vec![
            (live, current.as_uuid(), 3),
            (live, older, 2),
            (closed, older, 5),
        ],
        ..DurableSnapshot::default()
    };

    // Act
    let snapshot = compose(registrations, durable);

    // Assert: every registration, with no session while opening or closed;
    // one window, counting only the attached connection's current session.
    assert_eq!(snapshot.inventory, vec![live, opening, closed]);
    assert_eq!(
        snapshot.sessions,
        vec![
            SessionRecord {
                session: Some(current.as_uuid()),
                peer,
                connection: live,
                registration: RegistrationState::Attached,
            },
            SessionRecord {
                session: None,
                peer,
                connection: opening,
                registration: RegistrationState::Opening,
            },
            SessionRecord {
                session: None,
                peer,
                connection: closed,
                registration: RegistrationState::Closed,
            },
        ]
    );
    assert_eq!(
        snapshot.windows,
        vec![WindowRecord {
            connection: live,
            in_flight: 3,
        }]
    );
    Ok(())
}

#[expect(
    clippy::panic_in_result_fn,
    reason = "Assertions must fail this test while fixture errors propagate through Result."
)]
#[test]
fn registrations_that_changed_across_the_durable_read_refuse_the_snapshot() -> Result<(), TestError>
{
    // Arrange: a connection attached before the read and closed after it.
    let peer = PeerId::new(Uuid::now_v7())?;
    let connection = ConnectionId::new(Uuid::now_v7())?;
    let before = vec![RegistrationRow {
        connection,
        peer,
        registration: attached(SessionId::new(Uuid::now_v7())?),
    }];
    let after = vec![RegistrationRow {
        connection,
        peer,
        registration: ConnectionRegistration::Closed,
    }];

    // Act
    let changed = super::fenced(before.clone(), DurableSnapshot::default(), &after);
    let unchanged = super::fenced(before.clone(), DurableSnapshot::default(), &before);

    // Assert
    assert_eq!(changed, Err(super::SnapshotRefusal::Unstable));
    assert_eq!(
        unchanged.map(|snapshot| snapshot.inventory),
        Ok(vec![connection])
    );
    Ok(())
}
