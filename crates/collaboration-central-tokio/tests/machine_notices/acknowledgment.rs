//! A Router that negotiated NoticesApplied is resent what it has not applied
//! and reconnects from what it applied; a central that does not honor the
//! capability neither advertises it nor owes anything.
use super::{
    attempt_row,
    session_fixture::{Fixture, TestError},
    support::{assert_hint, deliver, invocation, next, notice},
};
use board_sync_protocol::BoardConsumerProtocol;
use collaboration_central_core::{CENTRAL_FRAME_CAPABILITIES, ObjectEvent, ObjectKernel};
use collaboration_central_tokio::SocketCommand;
use fleet_protocol::{
    ClientFrame, ConnectionId, FrameCapabilities, FrameId, NoticePosition, OperationAnswer,
    OperationRequest, PeerId, SchemaSupport, ServerFrame, SessionId,
};
use fleet_session::{ConnectionBinding, VerifiedPeer};
use runtime_primitives::{Clock, SyncTime};
use tokio::sync::mpsc;

const NOTICE_DELIVERY: &str = "notice_delivery";

/// Socket events go through the object kernel, as the shells deliver them, so
/// the post-event notice push runs after each one.
async fn event(fixture: &Fixture, event: ObjectEvent) {
    ObjectKernel::new(&fixture.runtime).on_event(event).await;
}

/// Open a connection that offers `offered`; answers the binding, its socket
/// and the capabilities the Welcome advertised.
async fn admit_offering(
    fixture: &Fixture,
    peer: PeerId,
    offered: Option<FrameCapabilities>,
) -> Result<
    (
        ConnectionBinding,
        mpsc::Receiver<SocketCommand>,
        Option<FrameCapabilities>,
    ),
    TestError,
> {
    fixture.enroll(peer).await?;
    let binding = ConnectionBinding::new(
        ConnectionId::new(durable_steps::Uuid::now_v7())?,
        VerifiedPeer {
            peer,
            expires_at: SyncTime::from_unix_millis(
                fixture
                    .io
                    .now()
                    .unix_millis()
                    .checked_add(3_600_000)
                    .ok_or("credential expiry overflow")?,
            ),
        },
    );
    let origin = collaboration_central_core::AdmissionOrigin {
        installation: message_board::ServiceId::try_from(
            "019f0000-0000-7000-8000-000000000001".to_owned(),
        )?,
        machine: "notice fixture".to_owned().try_into()?,
    };
    let mut socket = fixture.host.native.attach_socket(binding, origin).await;
    event(fixture, ObjectEvent::SocketOpened { binding }).await;
    let session = SessionId::new(durable_steps::Uuid::now_v7())?;
    let hello = ClientFrame::<BoardConsumerProtocol>::Hello {
        capabilities: offered,
        session_id: session,
        schema_support: SchemaSupport { max_version: 7 },
    };
    event(
        fixture,
        ObjectEvent::Frame {
            binding,
            frame: serde_json::to_vec(&hello)?,
        },
    )
    .await;
    crate::kernel_alarm::deliver_alarm(&fixture.runtime).await?;
    let ServerFrame::Welcome {
        session_id,
        capabilities,
        ..
    } = next(&mut socket).await?
    else {
        return Err("the connection was not welcomed first".into());
    };
    if session_id != session {
        return Err("the Welcome named another session".into());
    }
    Ok((binding, socket, capabilities))
}

/// Send NoticesApplied and answer the position central recorded.
async fn apply(
    fixture: &Fixture,
    binding: ConnectionBinding,
    socket: &mut mpsc::Receiver<SocketCommand>,
    through: u64,
) -> Result<u64, TestError> {
    let frame = FrameId::new(durable_steps::Uuid::now_v7())?;
    let request = ClientFrame::<BoardConsumerProtocol>::Request {
        frame,
        body: OperationRequest::NoticesApplied {
            through: NoticePosition::new(through),
        },
    };
    event(
        fixture,
        ObjectEvent::Frame {
            binding,
            frame: serde_json::to_vec(&request)?,
        },
    )
    .await;
    let ServerFrame::Answer {
        frame: actual,
        body: OperationAnswer::NoticesApplied { through },
    } = next(socket).await?
    else {
        return Err("NoticesApplied did not yield its typed answer".into());
    };
    if actual != frame {
        return Err("NoticesApplied answered another frame".into());
    }
    Ok(through.value())
}

#[expect(
    clippy::panic_in_result_fn,
    reason = "Assertions compare the Welcome, the owed rows and the socket against an unhonored capability."
)]
#[tokio::test]
async fn a_central_that_does_not_honor_notices_applied_neither_advertises_nor_owes_it()
-> Result<(), TestError> {
    let root = tempfile::tempdir()?;
    let fixture = Fixture::open(root.path()).await?;
    let peer = PeerId::new(durable_steps::Uuid::now_v7())?;
    let (_, mut socket, advertised) =
        admit_offering(&fixture, peer, Some(FrameCapabilities::SUPPORTED)).await?;
    assert!(
        !advertised.is_some_and(|capabilities| capabilities.notices_applied),
        "an unhonored capability was advertised: {advertised:?}"
    );
    let first = notice(1)?;
    deliver(&fixture, &invocation(&first)?).await?;
    assert_hint(next(&mut socket).await?, 1, 1, &first);
    assert_eq!(attempt_row(&fixture, peer, NOTICE_DELIVERY).await?, None);
    fixture.io.clock.set_offset(10 * 60_000);
    crate::kernel_alarm::deliver_alarm(&fixture.runtime).await?;
    assert!(
        socket.try_recv().is_err(),
        "an unacknowledging Router was resent"
    );
    fixture.objects.close().await?;
    Ok(())
}

#[expect(
    clippy::panic_in_result_fn,
    reason = "Assertions follow one owed notice through its resend, acknowledgment and the reconnect catch-up."
)]
#[tokio::test]
async fn a_negotiated_router_is_resent_until_it_applies_and_reconnects_from_what_it_applied()
-> Result<(), TestError> {
    let root = tempfile::tempdir()?;
    let fixture = Fixture::open_honoring(root.path(), CENTRAL_FRAME_CAPABILITIES).await?;
    let peer = PeerId::new(durable_steps::Uuid::now_v7())?;
    let (binding, mut socket, advertised) =
        admit_offering(&fixture, peer, Some(FrameCapabilities::SUPPORTED)).await?;
    assert_eq!(advertised, Some(CENTRAL_FRAME_CAPABILITIES));
    let first = notice(1)?;
    deliver(&fixture, &invocation(&first)?).await?;
    assert_hint(next(&mut socket).await?, 1, 1, &first);
    let owed = attempt_row(&fixture, peer, NOTICE_DELIVERY).await?;
    assert!(
        matches!(&owed, Some((state, 0, None, Some(_))) if state == "waiting"),
        "{owed:?}"
    );
    // Unanswered past the first retry: the notice goes again on the next alarm.
    fixture.io.clock.set_offset(30_001);
    crate::kernel_alarm::deliver_alarm(&fixture.runtime).await?;
    assert_hint(next(&mut socket).await?, 2, 1, &first);
    let retried = attempt_row(&fixture, peer, NOTICE_DELIVERY).await?;
    assert!(
        matches!(&retried, Some((_, 1, Some(class), _)) if class == "unanswered"),
        "{retried:?}"
    );
    // An acknowledgment past the issued head is clamped to it, and settles the row.
    assert_eq!(apply(&fixture, binding, &mut socket, 5).await?, 1);
    assert_eq!(attempt_row(&fixture, peer, NOTICE_DELIVERY).await?, None);
    fixture.io.clock.set_offset(10 * 60_000);
    crate::kernel_alarm::deliver_alarm(&fixture.runtime).await?;
    assert!(socket.try_recv().is_err(), "an applied notice was resent");
    // Disconnected, the next notice waits; a new session starts after what was applied.
    event(&fixture, ObjectEvent::SocketClosed { binding }).await;
    let second = notice(2)?;
    deliver(&fixture, &invocation(&second)?).await?;
    let (_, mut socket, _) =
        admit_offering(&fixture, peer, Some(FrameCapabilities::SUPPORTED)).await?;
    assert_hint(next(&mut socket).await?, 1, 2, &second);
    assert!(
        socket.try_recv().is_err(),
        "the reconnect repeated a notice"
    );
    fixture.objects.close().await?;
    Ok(())
}
