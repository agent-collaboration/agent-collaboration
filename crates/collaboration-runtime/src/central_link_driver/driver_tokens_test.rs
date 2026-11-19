//! The driver against a fake B3 token source: NeedsNewCode stops retrying and
//! reports not enrolled until the source changes, RefreshPending waits, a new
//! token renews the open connection, and every renewal result is reported.
use super::super::super::driver_test_support::*;
use super::super::super::*;
use central_link_client::{LinkCalls, LinkStatus, RefusalReason, Submission, SubmitRefusal};
use central_link_protocol::{DataFrame, LinkFrame, TokenRenewalRefusal, TokenRenewalResult};
use std::time::Duration;

/// A status read is one round trip through the driver loop.
async fn settle(harness: &Harness) -> Option<LinkStatus> {
    harness.calls.link_status().await
}

fn not_enrolled() -> Option<LinkStatus> {
    Some(LinkStatus::Refused {
        reason: RefusalReason::CredentialRefused,
    })
}

#[tokio::test]
async fn needs_new_code_stops_retrying_and_reports_not_enrolled_until_the_source_changes() {
    let mut harness = Harness::start_with_tokens(
        tempfile::tempdir().unwrap(),
        clock_at(MARK - 20_000),
        TokenAvailability::NeedsNewCode,
    )
    .await;
    let status = settle(&harness).await;
    // Ten minutes of injected time: a retrying link would have tried many times.
    for _ in 0..10 {
        harness.clock.advance(Duration::from_secs(60)).unwrap();
        settle(&harness).await;
    }
    let immediate = harness.calls.submit(caller(), list_areas()).await;
    let queued = harness
        .calls
        .submit(caller(), thread_post(&uuid_v7(7)))
        .await;
    let connect_attempts = harness
        .tokens
        .reads
        .load(std::sync::atomic::Ordering::SeqCst);
    assert_eq!(status, not_enrolled());
    assert_eq!(connect_attempts, 1, "no retry while NeedsNewCode");
    assert!(matches!(
        immediate,
        Submission::Refused(SubmitRefusal::NotEnrolled)
    ));
    assert!(
        matches!(queued, Submission::InOutbox(_)),
        "outbox writes wait"
    );

    harness.tokens.set(TokenAvailability::Ready(fixture_token(
        "fresh.machine.token",
    )));
    let central = harness.connected().await;
    let after = settle(&harness).await;

    assert!(matches!(after, Some(LinkStatus::Connected { .. })));
    drop(central);
    harness.stop().await;
}

#[tokio::test]
async fn refresh_pending_does_not_connect_and_a_ready_token_connects_on_the_next_retry() {
    let mut harness = Harness::start_with_tokens(
        tempfile::tempdir().unwrap(),
        clock_at(MARK - 20_000),
        TokenAvailability::RefreshPending,
    )
    .await;
    settle(&harness).await;
    harness.clock.advance(Duration::from_secs(1)).unwrap();
    let status = settle(&harness).await;
    let attempted = harness.connections.try_recv().is_ok();

    harness.tokens.set(TokenAvailability::Ready(fixture_token(
        "fixture.machine.token",
    )));
    // The backoff doubled to 2 s after the second pending read.
    harness.clock.advance(Duration::from_secs(2)).unwrap();
    let central = harness.connected().await;

    assert!(!attempted, "RefreshPending opens no socket");
    assert_eq!(status, Some(LinkStatus::Connecting));
    assert!(matches!(
        settle(&harness).await,
        Some(LinkStatus::Connected { .. })
    ));
    drop(central);
    harness.stop().await;
}

#[tokio::test]
async fn a_new_token_renews_the_open_connection_and_every_result_is_reported() {
    let mut harness = Harness::start(tempfile::tempdir().unwrap(), clock_at(MARK - 20_000)).await;
    let mut central = harness.connected().await;
    let renewed = TokenRenewalResult::Renewed {
        effective_expires_at: chrono::DateTime::from_timestamp_millis(MARK + 3_600_000).unwrap(),
    };
    let refused = TokenRenewalResult::Refused {
        reason: TokenRenewalRefusal::TokenRejected,
    };

    let mut sent = Vec::new();
    for (token, answer) in [
        ("second.machine.token", &renewed),
        ("third.machine.token", &refused),
    ] {
        harness
            .tokens
            .set(TokenAvailability::Ready(fixture_token(token)));
        let LinkFrame::Data(DataFrame::RenewMachineToken {
            correlation_id,
            token,
        }) = central.frame().await
        else {
            panic!("a RenewMachineToken frame");
        };
        sent.push(token.expose().to_owned());
        central.send(LinkFrame::Data(DataFrame::MachineTokenRenewal {
            correlation_id,
            result: answer.clone(),
        }));
        settle(&harness).await;
    }

    assert_eq!(sent, ["second.machine.token", "third.machine.token"]);
    assert_eq!(*harness.tokens.renewals.lock().unwrap(), [renewed, refused]);
    assert!(matches!(
        settle(&harness).await,
        Some(LinkStatus::Connected { .. })
    ));
    harness.stop().await;
}

#[tokio::test]
async fn the_token_the_connection_already_carries_is_not_renewed_again() {
    let mut harness = Harness::start(tempfile::tempdir().unwrap(), clock_at(MARK - 20_000)).await;
    let mut central = harness.connected().await;

    harness.tokens.set(TokenAvailability::Ready(fixture_token(
        "fixture.machine.token",
    )));
    settle(&harness).await;
    // The next frame the driver sends is the immediate request, not a renewal.
    let call = tokio::spawn({
        let calls = harness.calls.clone();
        async move { calls.submit(caller(), list_areas()).await }
    });
    let frame = central.frame().await;

    assert!(matches!(frame, LinkFrame::Data(DataFrame::Request { .. })));
    call.abort();
    harness.stop().await;
}

#[tokio::test]
async fn central_unavailable_is_transient_and_the_renewal_is_sent_again_until_it_succeeds() {
    let mut harness = Harness::start(tempfile::tempdir().unwrap(), clock_at(MARK - 20_000)).await;
    let mut central = harness.connected().await;
    let unavailable = TokenRenewalResult::Refused {
        reason: TokenRenewalRefusal::CentralUnavailable,
    };
    let renewed = TokenRenewalResult::Renewed {
        effective_expires_at: chrono::DateTime::from_timestamp_millis(MARK + 3_600_000).unwrap(),
    };
    harness.tokens.set(TokenAvailability::Ready(fixture_token(
        "second.machine.token",
    )));

    let mut sent = Vec::new();
    for answer in [&unavailable, &renewed] {
        let LinkFrame::Data(DataFrame::RenewMachineToken {
            correlation_id,
            token,
        }) = central.frame().await
        else {
            panic!("a RenewMachineToken frame");
        };
        sent.push(token.expose().to_owned());
        central.send(LinkFrame::Data(DataFrame::MachineTokenRenewal {
            correlation_id,
            result: answer.clone(),
        }));
        let status = settle(&harness).await;
        assert!(
            matches!(status, Some(LinkStatus::Connected { .. })),
            "the link stays up and enrolled after {answer:?}"
        );
        // The retry delay, on the injected clock; the source never changed.
        harness.clock.advance(Duration::from_secs(5)).unwrap();
    }

    assert_eq!(sent, ["second.machine.token", "second.machine.token"]);
    assert_eq!(
        *harness.tokens.renewals.lock().unwrap(),
        [unavailable, renewed]
    );
    harness.stop().await;
}

#[tokio::test]
async fn an_authority_lapsed_close_reconnects_only_with_a_fresh_token() {
    let mut harness = Harness::start(tempfile::tempdir().unwrap(), clock_at(MARK - 20_000)).await;
    let central = harness.connected().await;
    let first_token = central.token.clone();

    central.close_lapsed();
    settle(&harness).await;
    // A minute of reconnect backoff: the source still offers the lapsed token.
    for _ in 0..6 {
        harness.clock.advance(Duration::from_secs(10)).unwrap();
        settle(&harness).await;
    }
    let reconnected_with_lapsed = harness.connections.try_recv().is_ok();
    assert!(
        !reconnected_with_lapsed,
        "the lapsed token is not used to reconnect"
    );
    assert_eq!(settle(&harness).await, Some(LinkStatus::Connecting));

    harness.tokens.set(TokenAvailability::Ready(fixture_token(
        "fresh.machine.token",
    )));
    harness.clock.advance(Duration::from_secs(60)).unwrap();
    let fresh = harness.connected().await;

    assert_eq!(first_token.expose(), "fixture.machine.token");
    assert_eq!(fresh.token.expose(), "fresh.machine.token");
    assert!(matches!(
        settle(&harness).await,
        Some(LinkStatus::Connected { .. })
    ));
    harness.stop().await;
}

#[tokio::test]
async fn any_other_close_reconnects_with_the_same_token() {
    let mut harness = Harness::start(tempfile::tempdir().unwrap(), clock_at(MARK - 20_000)).await;
    let central = harness.connected().await;

    drop(central);
    settle(&harness).await;
    harness.clock.advance(Duration::from_secs(1)).unwrap();
    let again = harness.connected().await;

    assert_eq!(again.token.expose(), "fixture.machine.token");
    harness.stop().await;
}
