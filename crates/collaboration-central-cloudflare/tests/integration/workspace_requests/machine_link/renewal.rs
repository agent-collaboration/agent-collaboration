//! R40/V32 on the hibernating Worker socket: a RenewMachineToken renews the open
//! connection, its rewritten attachment survives eviction, and a refusal keeps
//! the old expiry.
use super::fixture::{self, Fixture, INSTALLATION, ORGANIZATION};
use super::{baseline, create_area};
use central_link_protocol::{DataFrame, TokenRenewalRefusal, TokenRenewalResult};
use collaboration_central_conformance::{ConformanceResult as Result, signin_fixtures as signed};
use runtime_primitives::Clock;
use std::time::Duration;

fn renew(correlation: &str, token: &str) -> Result<DataFrame> {
    Ok(DataFrame::RenewMachineToken {
        correlation_id: correlation.parse()?,
        token: token.to_owned().try_into()?,
    })
}

async fn renewal(link: &mut super::ConnectedLink, frame: DataFrame) -> Result<TokenRenewalResult> {
    fixture::send(&mut link.socket, &frame).await?;
    match fixture::receive(&mut link.socket).await? {
        DataFrame::MachineTokenRenewal { result, .. } => Ok(result),
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
