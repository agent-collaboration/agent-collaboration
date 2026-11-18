//! R40/V32 on the hibernating Worker socket: a RenewMachineToken renews the open
//! connection, its rewritten attachment survives eviction, and a refusal keeps
//! the old expiry.
use super::fixture::{self, Fixture, INSTALLATION, ORGANIZATION};
use super::{baseline, create_area};
use central_link_protocol::RequestPayload;
use central_link_protocol::{DataFrame, TokenRenewalRefusal, TokenRenewalResult};
use collaboration_central_conformance::{ConformanceResult as Result, signin_fixtures as signed};
use futures_util::StreamExt;
use reqwest_websocket::{CloseCode, Message};
use runtime_primitives::Clock;
use std::time::Duration;
use workspace_model::{
    RefusalReason, WorkspaceId, WorkspaceOperation, WorkspaceRequest, WorkspaceResult,
};

fn renew(correlation: &str, token: &str) -> Result<DataFrame> {
    Ok(DataFrame::RenewMachineToken {
        correlation_id: correlation.parse()?,
        token: token.to_owned().try_into()?,
    })
}

async fn renewal(link: &mut super::ConnectedLink, frame: DataFrame) -> Result<TokenRenewalResult> {
    let DataFrame::RenewMachineToken {
        correlation_id: sent,
        ..
    } = &frame
    else {
        return Err("renewal() sends a RenewMachineToken".into());
    };
    let sent = sent.clone();
    fixture::send(&mut link.socket, &frame).await?;
    match fixture::receive(&mut link.socket).await? {
        DataFrame::MachineTokenRenewal {
            correlation_id,
            result,
        } if correlation_id == sent => Ok(result),
        other => Err(format!("expected a renewal answer, got {other:?}").into()),
    }
}

#[tokio::test]
async fn renewal_on_the_open_socket_survives_eviction_and_runs_past_the_first_token() -> Result<()>
{
    let fixture = Fixture::new().await?;
    let result = renewal_journey(&fixture).await;
    fixture.close().await?;
    result
}

async fn renewal_journey(fixture: &Fixture) -> Result<()> {
    // Arrange: the open socket's first token counts for 600 s.
    let (mut link, _, workspace, _, _) = baseline(fixture).await?;

    // A token that fails verification is refused; the socket keeps its expiry.
    let rejected = renewal(
        &mut link,
        renew("019f0000-0000-7000-8000-00000000c301", "not.a.token")?,
    )
    .await?;
    if rejected
        != (TokenRenewalResult::Refused {
            reason: TokenRenewalRefusal::TokenRejected,
        })
    {
        return Err(format!("a rejected token must be refused: {rejected:?}").into());
    }

    // Act: at 300 s the Router renews on the open socket with a fresh token
    // whose grant runs to 900 s.
    fixture.clock.advance(Duration::from_secs(300))?;
    fixture
        .host
        .set_external_time_ms(fixture.clock.now().unix_millis())
        .await?;
    let _: bool = fixture
        .runtime()
        .control("clock", &fixture.clock.now().unix_millis())
        .await
        .unwrap();
    let token = signed::machine_token(
        ORGANIZATION,
        INSTALLATION,
        fixture.enrollment.as_str(),
        signed::NOW_SECONDS + 300,
        signed::NOW_SECONDS + 900,
    )?;
    let renewed = renewal(
        &mut link,
        renew("019f0000-0000-7000-8000-00000000c302", &token)?,
    )
    .await?;
    let expected =
        chrono::DateTime::from_timestamp(signed::NOW_SECONDS + 900, 0).ok_or("renewed expiry")?;
    if renewed
        != (TokenRenewalResult::Renewed {
            effective_expires_at: expected,
        })
    {
        return Err(format!("the fresh token must renew the socket: {renewed:?}").into());
    }

    // Assert: after eviction, past the first token's 600 s, the restored socket
    // still admits a write; the renewed expiry came back from the attachment.
    fixture.host.c2_evict(&fixture.organization).await?;
    fixture.clock.advance(Duration::from_secs(400))?;
    fixture
        .host
        .set_external_time_ms(fixture.clock.now().unix_millis())
        .await?;
    let _: bool = fixture
        .runtime()
        .control("clock", &fixture.clock.now().unix_millis())
        .await
        .unwrap();
    let exchange = link
        .call_as(fixture::session_caller()?, create_area(&workspace)?)
        .await?;
    let admitted = matches!(
        &exchange.reply,
        DataFrame::Reply { result, .. } if matches!(
            result.as_ref(),
            central_link_protocol::ReplyPayload::Workspace(reply)
                if matches!(reply.result, workspace_model::WorkspaceResult::Done(_))
        )
    );
    if !admitted {
        return Err(format!(
            "the renewed socket must admit the write: {:?}",
            exchange.reply
        )
        .into());
    }
    drop(link);
    Ok(())
}

const OTHER_INSTALLATION: &str = "019f0000-0000-7000-8000-00000000cf2a";
const OTHER_ENROLLMENT: &str = "019f0000-0000-7000-8000-00000000cf2b";

/// Moves the one injected clock and hands the time to the Worker and its object.
async fn advance(fixture: &Fixture, seconds: u64) -> Result<()> {
    fixture.clock.advance(Duration::from_secs(seconds))?;
    fixture
        .host
        .set_external_time_ms(fixture.clock.now().unix_millis())
        .await?;
    let _: bool = fixture
        .runtime()
        .control("clock", &fixture.clock.now().unix_millis())
        .await
        .unwrap();
    Ok(())
}

/// A fresh token for the fixture's enrollment, decided at `now` with a grant to `until`.
fn fresh(fixture: &Fixture, now: i64, until: i64) -> Result<String> {
    signed::machine_token(
        ORGANIZATION,
        INSTALLATION,
        fixture.enrollment.as_str(),
        signed::NOW_SECONDS + now,
        signed::NOW_SECONDS + until,
    )
}

fn not_active() -> TokenRenewalResult {
    TokenRenewalResult::Refused {
        reason: TokenRenewalRefusal::EnrollmentNotActive,
    }
}

fn area(request_id: &str, name: &str, workspace: &WorkspaceId) -> Result<WorkspaceRequest> {
    Ok(WorkspaceRequest {
        request_id: Some(request_id.parse()?),
        workspace_id: Some(workspace.clone()),
        operation: WorkspaceOperation::CreateArea {
            name: name.to_owned().try_into()?,
            description: "Test description".to_owned().try_into().unwrap(),
        },
    })
}

fn is_done(frame: &DataFrame) -> bool {
    matches!(
        frame,
        DataFrame::Reply { result, .. } if matches!(
            result.as_ref(),
            central_link_protocol::ReplyPayload::Workspace(reply)
                if matches!(reply.result, workspace_model::WorkspaceResult::Done(_))
        )
    )
}

#[tokio::test]
async fn a_renewal_just_before_the_deadline_renews_and_at_or_after_it_is_refused() -> Result<()> {
    let fixture = Fixture::new().await?;
    let result = deadline_journey(&fixture).await;
    fixture.close().await?;
    result
}

async fn deadline_journey(fixture: &Fixture) -> Result<()> {
    // Arrange: the open socket counts until +600.
    let (mut link, _, _, _, _) = baseline(fixture).await?;

    // Act: one second before the deadline a fresh token renews to +1199; at
    // the renewed deadline and after it, a still-valid fresh token is refused.
    advance(fixture, 599).await?;
    let just_before = renewal(
        &mut link,
        renew(
            "019f0000-0000-7000-8000-00000000c311",
            &fresh(fixture, 599, 1199)?,
        )?,
    )
    .await?;
    advance(fixture, 600).await?;
    let late = fresh(fixture, 1199, 1799)?;
    let at_deadline = renewal(
        &mut link,
        renew("019f0000-0000-7000-8000-00000000c312", &late)?,
    )
    .await?;
    advance(fixture, 1).await?;
    let after = renewal(
        &mut link,
        renew("019f0000-0000-7000-8000-00000000c313", &late)?,
    )
    .await?;

    // Assert
    let renewed_to =
        chrono::DateTime::from_timestamp(signed::NOW_SECONDS + 1199, 0).ok_or("renewed expiry")?;
    if just_before
        != (TokenRenewalResult::Renewed {
            effective_expires_at: renewed_to,
        })
    {
        return Err(format!("one second before the deadline must renew: {just_before:?}").into());
    }
    if at_deadline != not_active() || after != not_active() {
        return Err(format!(
            "at and after the deadline renewal is refused: {at_deadline:?}, {after:?}"
        )
        .into());
    }
    drop(link);
    Ok(())
}

#[tokio::test]
async fn every_refused_renewal_keeps_the_deadline_through_eviction() -> Result<()> {
    let fixture = Fixture::new().await?;
    let result = refusals_journey(&fixture).await;
    fixture.close().await?;
    result
}

async fn refusals_journey(fixture: &Fixture) -> Result<()> {
    // Arrange: the open socket counts until +600.
    let (mut link, _, workspace, _, _) = baseline(fixture).await?;
    let other_machine = signed::machine_token(
        ORGANIZATION,
        OTHER_INSTALLATION,
        fixture.enrollment.as_str(),
        signed::NOW_SECONDS,
        signed::NOW_SECONDS + 900,
    )?;
    let other_enrollment = signed::machine_token(
        ORGANIZATION,
        INSTALLATION,
        OTHER_ENROLLMENT,
        signed::NOW_SECONDS,
        signed::NOW_SECONDS + 900,
    )?;

    // Act: three refusals and no successful renewal; then the object is evicted.
    let mut answers = Vec::new();
    for (correlation, token) in [
        ("019f0000-0000-7000-8000-00000000c321", "not.a.token"),
        (
            "019f0000-0000-7000-8000-00000000c322",
            other_machine.as_str(),
        ),
        (
            "019f0000-0000-7000-8000-00000000c323",
            other_enrollment.as_str(),
        ),
    ] {
        answers.push(renewal(&mut link, renew(correlation, token)?).await?);
    }
    fixture.host.c2_evict(&fixture.organization).await?;
    // The restored attachment still carries +600: a write at +599 lands, and
    // at +600 even a valid fresh token cannot renew it.
    advance(fixture, 599).await?;
    let before = link
        .call_as(
            fixture::session_caller()?,
            area(
                "019f0000-0000-7000-8000-00000000cf31",
                "Before the deadline",
                &workspace,
            )?,
        )
        .await?;
    advance(fixture, 1).await?;
    let at_deadline = renewal(
        &mut link,
        renew(
            "019f0000-0000-7000-8000-00000000c324",
            &fresh(fixture, 600, 1200)?,
        )?,
    )
    .await?;

    // Assert
    let expected = [
        TokenRenewalRefusal::TokenRejected,
        TokenRenewalRefusal::DifferentMachine,
        TokenRenewalRefusal::DifferentEnrollment,
    ]
    .map(|reason| TokenRenewalResult::Refused { reason });
    if answers != expected {
        return Err(format!("each refusal carries its reason: {answers:?}").into());
    }
    if !is_done(&before.reply) {
        return Err(format!("the old deadline still admits +599: {:?}", before.reply).into());
    }
    if at_deadline != not_active() {
        return Err(format!("the old deadline still ends at +600: {at_deadline:?}").into());
    }
    drop(link);
    Ok(())
}

#[tokio::test]
async fn a_renewal_after_the_enrollment_is_superseded_is_refused_as_not_active() -> Result<()> {
    let fixture = Fixture::new().await?;
    let result = superseded_journey(&fixture).await;
    fixture.close().await?;
    result
}

async fn superseded_journey(fixture: &Fixture) -> Result<()> {
    let (mut link, _, _, _, _) = baseline(fixture).await?;
    let token = fresh(fixture, 0, 900)?;
    fixture.supersede().await?;

    let answer = renewal(
        &mut link,
        renew("019f0000-0000-7000-8000-00000000c331", &token)?,
    )
    .await?;

    if answer != not_active() {
        return Err(format!("a superseded enrollment cannot renew: {answer:?}").into());
    }
    drop(link);
    Ok(())
}

#[tokio::test]
async fn a_batch_after_the_deadline_refuses_every_member_and_then_closes() -> Result<()> {
    let fixture = Fixture::new().await?;
    let result = lapsed_batch_journey(&fixture).await;
    fixture.close().await?;
    result
}

async fn lapsed_batch_journey(fixture: &Fixture) -> Result<()> {
    let (mut link, _, workspace, _, _) = baseline(fixture).await?;
    advance(fixture, 600).await?;

    // The batch helper checks each reply's correlation and order.
    let (_, replies) = link
        .batch(vec![
            RequestPayload::Workspace(area(
                "019f0000-0000-7000-8000-00000000cf41",
                "Lapsed member A",
                &workspace,
            )?),
            RequestPayload::Workspace(area(
                "019f0000-0000-7000-8000-00000000cf42",
                "Lapsed member B",
                &workspace,
            )?),
        ])
        .await?;
    let closed = link.socket.next().await;

    for reply in &replies {
        let refused = matches!(
            reply,
            DataFrame::Reply { result, .. } if matches!(
                result.as_ref(),
                central_link_protocol::ReplyPayload::Workspace(reply)
                    if reply.result == WorkspaceResult::Refused(RefusalReason::MachineRevoked)
            )
        );
        if !refused {
            return Err(format!("each lapsed member is refused: {reply:?}").into());
        }
    }
    match closed {
        Some(Ok(Message::Close {
            code: CloseCode::Policy,
            reason,
        })) if reason == "machine authority expired or revoked" => Ok(()),
        other => Err(format!("the lapsed link closes after the batch: {other:?}").into()),
    }
}

#[tokio::test]
async fn a_renewal_whose_key_cannot_be_fetched_is_central_unavailable() -> Result<()> {
    let fixture = Fixture::new().await?;
    let result = key_fetch_journey(&fixture).await;
    fixture.close().await?;
    result
}

async fn key_fetch_journey(fixture: &Fixture) -> Result<()> {
    use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
    let (mut link, _, workspace, _, _) = baseline(fixture).await?;
    fixture.keys().fail_fetches();
    // A fresh token whose header names a key central never cached.
    let fresh = fresh(fixture, 0, 900)?;
    let rest = fresh.split_once('.').ok_or("token header")?.1;
    let header =
        URL_SAFE_NO_PAD.encode(br#"{"alg":"EdDSA","typ":"at+jwt","kid":"stamp-uncached"}"#);

    let answer = renewal(
        &mut link,
        renew(
            "019f0000-0000-7000-8000-00000000c351",
            &format!("{header}.{rest}"),
        )?,
    )
    .await?;
    let still_open = link
        .call_as(
            fixture::session_caller()?,
            area(
                "019f0000-0000-7000-8000-00000000cf51",
                "Still open",
                &workspace,
            )?,
        )
        .await?;

    let unavailable = TokenRenewalResult::Refused {
        reason: TokenRenewalRefusal::CentralUnavailable,
    };
    if answer != unavailable {
        return Err(format!("an unreachable key set is CentralUnavailable: {answer:?}").into());
    }
    if !is_done(&still_open.reply) {
        return Err(format!("the link stays up: {:?}", still_open.reply).into());
    }
    drop(link);
    Ok(())
}
