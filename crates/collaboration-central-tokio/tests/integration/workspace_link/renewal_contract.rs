//! RenewMachineToken on the open native link (R40/V32): renewal extends the
//! connection; a refusal of any reason keeps its deadline; and a renewal is a
//! frame, refused once the current deadline has passed.
use super::fixture::{Fixture, INSTALLATION, OTHER_MACHINE, receive, send};
use super::link_support::{
    caller, connect_client, correlation, create_workspace, created_workspace, renewed_token,
    workspace_reply,
};
use central_link_protocol::{
    CorrelationId, DataFrame, LinkFrame, LinkRequest, LinkRequestBatch, ReplyPayload,
    TokenRenewalRefusal, TokenRenewalResult,
};
use central_storage::{SqlValue, Statement};
use collaboration_central_conformance::{ConformanceResult as Result, signin_fixtures as signed};
use collaboration_central_core::{
    MachineLoginControl, ValidatedMachineConnection, identity_contract::MachineTokenDecision,
    machine_login_control,
};
use futures_util::StreamExt;
use reqwest_websocket::{CloseCode, Message, WebSocket};
use serde_json::json;
use std::time::Duration;
use workspace_model::{RefusalReason, WorkspaceResult};

#[expect(
    clippy::panic_in_result_fn,
    reason = "Assertions compare real socket outcomes and owning rows with literal grant boundaries."
)]
#[tokio::test]
async fn renew_machine_token_extends_the_open_connection_and_a_refusal_keeps_its_expiry()
-> Result<()> {
    // Arrange: an ordinary client opens with the actual initial 600-second grant.
    let fixture = Fixture::new().await?;
    let (mut client, attempt, mut socket) = connect_client(&fixture, &fixture.token).await?;
    let runtime = fixture
        .host
        .central(&signed::ORGANIZATION.to_owned().try_into()?)
        .await?;
    let connection = runtime
        .admit_machine_connection(&fixture.token)
        .await?
        .map_err(|_| "initial admission refused")?;
    assert_eq!(
        connection.effective_expires_at().0,
        signed::NOW_SECONDS + 600
    );

    // A token that fails sign-in verification is refused; the old expiry stands.
    send(
        &mut socket,
        &renew_frame("019f0000-0000-7000-8000-00000000c201", "not.a.token")?,
    )
    .await?;
    assert_eq!(
        renewal_result(&mut socket).await?,
        TokenRenewalResult::Refused {
            reason: TokenRenewalRefusal::TokenRejected
        }
    );

    // Act: at 300 s sign-in decides a fresh grant (read live), and the Router
    // sends the fresh token on the open connection (R40/V32, B3 renewal).
    fixture.clock.advance(Duration::from_secs(300))?;
    let grant = runtime.decide_machine_token(&serde_json::from_value(json!({
        "organizationId":signed::ORGANIZATION,"enrollmentId":connection.enrollment_id(),"clientId":connection.client_id(),
    }))?).await?;
    assert!(
        matches!(&grant, MachineTokenDecision::Grant { decided_at, grant_expires_at, .. }
        if decided_at.0 == signed::NOW_SECONDS + 300 && grant_expires_at.0 == signed::NOW_SECONDS + 900)
    );
    let token = renewed_token(&fixture.token, &grant, connection.client_id().as_str())?;
    send(
        &mut socket,
        &renew_frame("019f0000-0000-7000-8000-00000000c202", &token)?,
    )
    .await?;
    assert_eq!(
        renewal_result(&mut socket).await?,
        TokenRenewalResult::Renewed {
            effective_expires_at: chrono::DateTime::from_timestamp(signed::NOW_SECONDS + 900, 0)
                .ok_or("renewed expiry")?,
        }
    );

    // Assert: the same socket runs past its first token's expiry, at 600 and
    // 899 s, and is refused at 900 s, the grant boundary with no leeway.
    for (advance, request_id, name) in [
        (
            300,
            "019f0000-0000-7000-8000-00000000d102",
            "Renewed at 600",
        ),
        (
            299,
            "019f0000-0000-7000-8000-00000000d103",
            "Renewed at 899",
        ),
    ] {
        fixture.clock.advance(Duration::from_secs(advance))?;
        send(&mut socket, &LinkFrame::Ping).await?;
        assert_eq!(receive(&mut socket).await?, LinkFrame::Pong);
        let request = client.begin_request(caller()?, create_workspace(request_id, name)?)?;
        send(&mut socket, &request).await?;
        let (reply_id, reply) = workspace_reply(&mut client, &attempt, &mut socket).await?;
        assert_eq!(reply_id, correlation(&request)?);
        assert_eq!(created_workspace(&reply)?.name.as_str(), name);
    }
    fixture.clock.advance(Duration::from_secs(1))?;
    let expired = client.begin_request(
        caller()?,
        create_workspace("019f0000-0000-7000-8000-00000000d104", "Expired renewal")?,
    )?;
    send(&mut socket, &expired).await?;
    let (_, reply) = workspace_reply(&mut client, &attempt, &mut socket).await?;
    assert_eq!(
        reply.result,
        WorkspaceResult::Refused(RefusalReason::MachineRevoked)
    );
    let rows = runtime
        .runtime()
        .statement_host()
        .read(Statement {
            sql: "SELECT name FROM directory_workspaces ORDER BY name",
            args: vec![],
        })
        .await?;
    assert_eq!(
        rows.rows,
        vec![
            vec![SqlValue::Text("Renewed at 600".into())],
            vec![SqlValue::Text("Renewed at 899".into())]
        ]
    );
    socket
        .close(reqwest_websocket::CloseCode::Normal, None)
        .await?;
    fixture.close().await
}

fn renew_frame(correlation_id: &str, token: &str) -> Result<LinkFrame> {
    Ok(LinkFrame::Data(DataFrame::RenewMachineToken {
        correlation_id: correlation_id.parse()?,
        token: token.to_owned().try_into()?,
    }))
}

async fn renewal_result(socket: &mut WebSocket) -> Result<TokenRenewalResult> {
    match receive(socket).await? {
        LinkFrame::Data(DataFrame::MachineTokenRenewal { result, .. }) => Ok(result),
        other => Err(format!("expected a renewal answer, got {other:?}").into()),
    }
}

/// Central's answer to one RenewMachineToken, checked against its correlation.
async fn answer_to(socket: &mut WebSocket, correlation: &str) -> Result<TokenRenewalResult> {
    let expected: CorrelationId = correlation.parse()?;
    match receive(socket).await? {
        LinkFrame::Data(DataFrame::MachineTokenRenewal {
            correlation_id,
            result,
        }) if correlation_id == expected => Ok(result),
        other => Err(format!("expected the renewal answer to {correlation}, got {other:?}").into()),
    }
}

/// A fresh machine token for the connection's enrollment, decided now by
/// sign-in (read live), for `client_id`.
async fn fresh_token(
    fixture: &Fixture,
    connection: &ValidatedMachineConnection,
    client_id: &str,
) -> Result<String> {
    let runtime = fixture
        .host
        .central(&signed::ORGANIZATION.to_owned().try_into()?)
        .await?;
    let grant = runtime
        .decide_machine_token(&serde_json::from_value(json!({
            "organizationId":signed::ORGANIZATION,"enrollmentId":connection.enrollment_id(),
            "clientId":connection.client_id(),
        }))?)
        .await?;
    renewed_token(&fixture.token, &grant, client_id)
}

fn not_active() -> TokenRenewalResult {
    TokenRenewalResult::Refused {
        reason: TokenRenewalRefusal::EnrollmentNotActive,
    }
}

fn at(seconds: i64) -> Result<chrono::DateTime<chrono::Utc>> {
    Ok(chrono::DateTime::from_timestamp(signed::NOW_SECONDS + seconds, 0).ok_or("UTC")?)
}

#[expect(
    clippy::panic_in_result_fn,
    reason = "Renewal answers and owning rows are compared at literal deadline boundaries."
)]
#[tokio::test]
async fn a_renewal_just_before_the_deadline_renews_and_at_or_after_it_is_refused() -> Result<()> {
    // Arrange: the connection counts until +600.
    let fixture = Fixture::new().await?;
    let (mut client, attempt, mut socket) = connect_client(&fixture, &fixture.token).await?;
    let runtime = fixture
        .host
        .central(&signed::ORGANIZATION.to_owned().try_into()?)
        .await?;
    let connection = runtime
        .admit_machine_connection(&fixture.token)
        .await?
        .map_err(|_| "initial admission refused")?;
    let client_id = connection.client_id().as_str().to_owned();

    // Act: one second before the deadline a fresh token renews to +1199.
    fixture.clock.advance(Duration::from_secs(599))?;
    let token = fresh_token(&fixture, &connection, &client_id).await?;
    send(
        &mut socket,
        &renew_frame("019f0000-0000-7000-8000-00000000c301", &token)?,
    )
    .await?;
    let just_before = answer_to(&mut socket, "019f0000-0000-7000-8000-00000000c301").await?;
    // At the renewed deadline, a still-valid fresh token cannot revive it.
    fixture.clock.advance(Duration::from_secs(600))?;
    let token = fresh_token(&fixture, &connection, &client_id).await?;
    send(
        &mut socket,
        &renew_frame("019f0000-0000-7000-8000-00000000c302", &token)?,
    )
    .await?;
    let at_deadline = answer_to(&mut socket, "019f0000-0000-7000-8000-00000000c302").await?;
    fixture.clock.advance(Duration::from_secs(1))?;
    send(
        &mut socket,
        &renew_frame("019f0000-0000-7000-8000-00000000c303", &token)?,
    )
    .await?;
    let after = answer_to(&mut socket, "019f0000-0000-7000-8000-00000000c303").await?;
    // A request after the deadline is answered MachineRevoked, then the link closes.
    let expired = client.begin_request(
        caller()?,
        create_workspace("019f0000-0000-7000-8000-00000000d301", "After the deadline")?,
    )?;
    send(&mut socket, &expired).await?;
    let (after_id, after_write) = workspace_reply(&mut client, &attempt, &mut socket).await?;
    let closed = socket.next().await;

    // Assert
    assert_eq!(
        just_before,
        TokenRenewalResult::Renewed {
            effective_expires_at: at(1199)?,
        }
    );
    assert_eq!(at_deadline, not_active());
    assert_eq!(
        after_write.result,
        WorkspaceResult::Refused(RefusalReason::MachineRevoked)
    );
    assert_eq!(after, not_active());
    assert_eq!(after_id, correlation(&expired)?);
    assert!(
        matches!(
            &closed,
            Some(Ok(Message::Close { code: CloseCode::Policy, reason }))
                if reason == "machine authority expired or revoked"
        ),
        "the lapsed link closes: {closed:?}"
    );
    let rows = runtime
        .runtime()
        .statement_host()
        .read(Statement {
            sql: "SELECT name FROM directory_workspaces",
            args: vec![],
        })
        .await?;
    assert!(rows.rows.is_empty(), "no write landed after the deadline");
    fixture.close().await
}

#[expect(
    clippy::panic_in_result_fn,
    reason = "Each refusal reason and the untouched deadline are observed on the real socket."
)]
#[tokio::test]
async fn every_refused_renewal_keeps_the_current_deadline() -> Result<()> {
    // Arrange: the connection counts until +600.
    let fixture = Fixture::new().await?;
    let (mut client, attempt, mut socket) = connect_client(&fixture, &fixture.token).await?;
    let runtime = fixture
        .host
        .central(&signed::ORGANIZATION.to_owned().try_into()?)
        .await?;
    let connection = runtime
        .admit_machine_connection(&fixture.token)
        .await?
        .map_err(|_| "initial admission refused")?;
    let other_machine = fixture.token_for_installation(OTHER_MACHINE)?;
    let other_client = fresh_token(
        &fixture,
        &connection,
        "019f0000-0000-7000-8000-00000000c3ff",
    )
    .await?;

    let overlong = signed::resign_machine_token(
        &fixture.token,
        &[("exp", serde_json::json!(signed::NOW_SECONDS + 601))],
    )?;
    // Act: refusals with no successful renewal anywhere in this test.
    let mut answers = Vec::new();
    for (correlation, token) in [
        ("019f0000-0000-7000-8000-00000000c311", "not.a.token"),
        (
            "019f0000-0000-7000-8000-00000000c312",
            other_machine.as_str(),
        ),
        (
            "019f0000-0000-7000-8000-00000000c313",
            other_client.as_str(),
        ),
        ("019f0000-0000-7000-8000-00000000c314", overlong.as_str()),
    ] {
        send(&mut socket, &renew_frame(correlation, token)?).await?;
        answers.push(answer_to(&mut socket, correlation).await?);
    }
    // The first deadline still decides: a write at +599 lands, at +600 it does not.
    let mut writes = Vec::new();
    for (advance, request_id, name) in [
        (
            599,
            "019f0000-0000-7000-8000-00000000d311",
            "Before the deadline",
        ),
        (1, "019f0000-0000-7000-8000-00000000d312", "At the deadline"),
    ] {
        fixture.clock.advance(Duration::from_secs(advance))?;
        send(&mut socket, &LinkFrame::Ping).await?;
        assert_eq!(receive(&mut socket).await?, LinkFrame::Pong);
        let request = client.begin_request(caller()?, create_workspace(request_id, name)?)?;
        send(&mut socket, &request).await?;
        let (reply_id, reply) = workspace_reply(&mut client, &attempt, &mut socket).await?;
        assert_eq!(reply_id, correlation(&request)?);
        writes.push(reply.result);
    }

    // Assert
    assert_eq!(
        answers,
        [
            TokenRenewalResult::Refused {
                reason: TokenRenewalRefusal::TokenRejected
            },
            TokenRenewalResult::Refused {
                reason: TokenRenewalRefusal::DifferentMachine
            },
            TokenRenewalResult::Refused {
                reason: TokenRenewalRefusal::TokenRejected
            },
            TokenRenewalResult::Refused {
                reason: TokenRenewalRefusal::TokenRejected
            },
        ]
    );
    assert!(matches!(&writes[0], WorkspaceResult::Done(_)));
    assert_eq!(
        writes[1],
        WorkspaceResult::Refused(RefusalReason::MachineRevoked)
    );
    let rows = runtime
        .runtime()
        .statement_host()
        .read(Statement {
            sql: "SELECT name FROM directory_workspaces",
            args: vec![],
        })
        .await?;
    assert_eq!(
        rows.rows,
        vec![vec![SqlValue::Text("Before the deadline".into())]]
    );
    socket
        .close(reqwest_websocket::CloseCode::Normal, None)
        .await?;
    fixture.close().await
}

#[expect(
    clippy::panic_in_result_fn,
    reason = "A live revocation must refuse a renewal that carries a valid fresh token."
)]
#[tokio::test]
async fn a_renewal_after_live_revocation_is_refused_as_enrollment_not_active() -> Result<()> {
    // Arrange: a valid fresh token, decided before the revocation.
    let fixture = Fixture::new().await?;
    let (_client, _attempt, mut socket) = connect_client(&fixture, &fixture.token).await?;
    let runtime = fixture
        .host
        .central(&signed::ORGANIZATION.to_owned().try_into()?)
        .await?;
    let connection = runtime
        .admit_machine_connection(&fixture.token)
        .await?
        .map_err(|_| "initial admission refused")?;
    let client_id = connection.client_id().as_str().to_owned();
    let token = fresh_token(&fixture, &connection, &client_id).await?;
    let revoked = machine_login_control(
        runtime.as_ref(),
        MachineLoginControl::Revoke {
            person_id: signed::OWNER.to_owned().try_into()?,
            operation_id: "019f0000-0000-7000-8000-00000000c321"
                .to_owned()
                .try_into()?,
            machine_id: INSTALLATION.to_owned().try_into()?,
            enrollment_id: connection.enrollment_id().clone(),
        },
    )
    .await?;

    // Act
    send(
        &mut socket,
        &renew_frame("019f0000-0000-7000-8000-00000000c322", &token)?,
    )
    .await?;
    let answer = answer_to(&mut socket, "019f0000-0000-7000-8000-00000000c322").await?;

    // Assert
    assert!(revoked.is_some());
    assert_eq!(answer, not_active());
    socket
        .close(reqwest_websocket::CloseCode::Normal, None)
        .await?;
    fixture.close().await
}

#[expect(
    clippy::panic_in_result_fn,
    reason = "Each batch member's refusal and the close are observed on the real socket."
)]
#[tokio::test]
async fn a_batch_after_the_deadline_refuses_every_member_and_then_closes() -> Result<()> {
    // Arrange: the connection counts until +600; two writes wait in one batch.
    let fixture = Fixture::new().await?;
    let (_client, _attempt, mut socket) = connect_client(&fixture, &fixture.token).await?;
    let members = [
        (
            "019f0000-0000-7000-8000-00000000c341",
            "019f0000-0000-7000-8000-00000000d341",
        ),
        (
            "019f0000-0000-7000-8000-00000000c342",
            "019f0000-0000-7000-8000-00000000d342",
        ),
    ]
    .into_iter()
    .map(|(correlation, request_id)| {
        Ok(LinkRequest::new(
            correlation.parse()?,
            caller()?,
            create_workspace(request_id, "Lapsed batch member")?,
        )?)
    })
    .collect::<Result<Vec<_>>>()?;
    fixture.clock.advance(Duration::from_secs(600))?;

    // Act
    send(
        &mut socket,
        &LinkFrame::Data(DataFrame::Batch {
            requests: LinkRequestBatch::new(members.clone())?,
        }),
    )
    .await?;
    let mut replies = Vec::new();
    for _ in &members {
        replies.push(receive(&mut socket).await?);
    }
    let closed = socket.next().await;

    // Assert
    for (member, reply) in members.iter().zip(&replies) {
        assert!(
            matches!(reply, LinkFrame::Data(DataFrame::Reply { correlation_id, result })
                if correlation_id == member.correlation_id()
                    && matches!(result.as_ref(), ReplyPayload::Workspace(reply)
                        if reply.result == WorkspaceResult::Refused(RefusalReason::MachineRevoked))),
            "each member is refused under its own correlation: {reply:?}"
        );
    }
    assert!(
        matches!(
            &closed,
            Some(Ok(Message::Close { code: CloseCode::Policy, reason }))
                if reason == "machine authority expired or revoked"
        ),
        "the lapsed link closes after the batch: {closed:?}"
    );
    fixture.close().await
}

/// `token` with its header naming a key central has never cached, so verifying
/// it needs a key-set fetch.
fn with_uncached_key(token: &str) -> Result<String> {
    use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
    let rest = token.split_once('.').ok_or("token header")?.1;
    let header =
        URL_SAFE_NO_PAD.encode(br#"{"alg":"EdDSA","typ":"at+jwt","kid":"stamp-uncached"}"#);
    Ok(format!("{header}.{rest}"))
}

#[expect(
    clippy::panic_in_result_fn,
    reason = "The renewal answer and the still-open link are observed on the real socket."
)]
#[tokio::test]
async fn a_renewal_whose_key_cannot_be_fetched_is_central_unavailable() -> Result<()> {
    // Arrange: sign-in's key set becomes unreachable.
    let fixture = Fixture::new().await?;
    let (_client, _attempt, mut socket) = connect_client(&fixture, &fixture.token).await?;
    fixture.keys().fail_fetches();
    let token = with_uncached_key(&fixture.token)?;

    // Act
    send(
        &mut socket,
        &renew_frame("019f0000-0000-7000-8000-00000000c351", &token)?,
    )
    .await?;
    let answer = answer_to(&mut socket, "019f0000-0000-7000-8000-00000000c351").await?;
    send(&mut socket, &LinkFrame::Ping).await?;
    let still_open = receive(&mut socket).await?;

    // Assert: transient, and the link stays up on its current connection.
    assert_eq!(
        answer,
        TokenRenewalResult::Refused {
            reason: TokenRenewalRefusal::CentralUnavailable
        }
    );
    assert_eq!(still_open, LinkFrame::Pong);
    fixture.close().await
}
