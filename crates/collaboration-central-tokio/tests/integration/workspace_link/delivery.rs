use super::*;
use central_link_protocol::{CorrelationId, LinkRequest};
use reqwest_websocket::WebSocket;
use workspace_model::{ResolvedSender, central_mail::CentralMailAnswer};

#[tokio::test]
async fn live_direct_mail_pushes_then_reconnect_owns_the_backlog_and_ack() -> Result<()> {
    let fixture = Fixture::new().await?;
    let result = delivery_journey(&fixture).await;
    fixture.close().await?;
    result
}

async fn delivery_journey(fixture: &Fixture) -> Result<()> {
    let session = workspace_ids::SessionRef {
        machine_id: INSTALLATION.parse()?,
        session_id: "workspace-link-delivery".to_owned().try_into()?,
    };
    let mut original = fixture.connect().await?;
    receive(&mut original).await?;
    let admitted = mail_reply(
        &mut original,
        &session,
        1,
        json!({"kind":"setNoticeCadence","cadence":"m15"}),
    )
    .await?;
    if !matches!(
        admitted,
        CentralMailResult::Done(CentralMailAnswer::NoticeCadenceSet { .. })
    ) {
        return Err("fixture session did not register with central".into());
    }
    let queued = mail_reply(
        &mut original,
        &session,
        2,
        json!({
            "kind":"sendMessage","to":session,"body":"deliver on the link",
            "delivery":"steer","replyTo":null,
            "sentAt":chrono::DateTime::from_timestamp(signed::NOW_SECONDS, 0).ok_or("fixture UTC")?
        }),
    )
    .await?;
    let CentralMailResult::Done(CentralMailAnswer::Queued { message_id }) = queued else {
        return Err("direct message was not accepted".into());
    };
    let first_offer = receive(&mut original).await?;
    if !matches!(&first_offer, LinkFrame::Data(DataFrame::Deliver {
        message, sender: ResolvedSender::Session { session: sender, display_name: None, machine_label: Some(label) },
    }) if message.message_id == message_id && message.target == session && sender == &session
        && label.as_str() == "Link test machine")
    {
        return Err("live delivery did not carry the accepted id and resolved sender".into());
    }

    let mut replacement = fixture.connect().await?;
    if !matches!(
        receive(&mut replacement).await?,
        LinkFrame::Data(DataFrame::Welcome { .. })
    ) {
        return Err("replacement did not receive Welcome first".into());
    }
    if receive(&mut replacement).await? != first_offer {
        return Err("replacement did not receive the same unacknowledged message".into());
    }
    drop(original);
    send(
        &mut replacement,
        &LinkFrame::Data(DataFrame::Ack {
            message_id: message_id.clone(),
            answer: central_link_protocol::DeliveryAnswer::Accepted,
        }),
    )
    .await?;
    send(&mut replacement, &LinkFrame::Ping).await?;
    if receive(&mut replacement).await? != LinkFrame::Pong {
        return Err("replacement link did not handle the Ack and remain usable".into());
    }
    let runtime = fixture
        .host
        .central(&signed::ORGANIZATION.to_owned().try_into()?)
        .await?;
    let rows = runtime
        .statement_host()
        .read(central_storage::Statement {
            sql: "SELECT state FROM mail_messages WHERE message_id=?",
            args: vec![central_storage::SqlValue::Text(message_id.to_string())],
        })
        .await?;
    if rows.rows != vec![vec![central_storage::SqlValue::Text("delivered".into())]] {
        return Err("the Ack did not settle the owning message row".into());
    }
    let next = mail_reply(
        &mut replacement,
        &session,
        3,
        json!({
            "kind":"sendMessage","to":session,"body":"deliver after the old socket closes",
            "delivery":"queue","replyTo":null,
            "sentAt":chrono::DateTime::from_timestamp(signed::NOW_SECONDS, 0).ok_or("fixture UTC")?
        }),
    )
    .await?;
    let CentralMailResult::Done(CentralMailAnswer::Queued {
        message_id: next_message,
    }) = next
    else {
        return Err("the replacement did not accept its next message".into());
    };
    if !matches!(receive(&mut replacement).await?, LinkFrame::Data(DataFrame::Deliver { message, .. }) if message.message_id == next_message)
    {
        return Err("closing the previous socket lost the registered replacement".into());
    }
    Ok(())
}

async fn mail_reply(
    socket: &mut WebSocket,
    session: &workspace_ids::SessionRef,
    index: u32,
    operation: serde_json::Value,
) -> Result<CentralMailResult> {
    let correlation: CorrelationId = format!("019f0000-0000-7000-8000-{index:012x}").parse()?;
    let request = LinkRequest::new(
        correlation.clone(),
        Caller::Session(session.clone()),
        RequestPayload::Mail(serde_json::from_value(json!({
            "requestId":format!("019f0000-0000-7000-8000-{:012x}",index+20),"operation":operation,
        }))?),
    )?;
    send(
        socket,
        &LinkFrame::Data(DataFrame::Request {
            request: Box::new(request),
        }),
    )
    .await?;
    let LinkFrame::Data(DataFrame::Reply {
        correlation_id,
        result,
    }) = receive(socket).await?
    else {
        return Err("expected central mail reply".into());
    };
    match *result {
        ReplyPayload::Mail(result) if correlation_id == correlation => Ok(result),
        _ => Err("expected matching mail reply".into()),
    }
}
