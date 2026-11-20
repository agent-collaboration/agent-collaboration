use super::*;
use central_link_protocol::RequestPayload;
use serde_json::json;
use workspace_model::{
    ResolvedSender, central_mail::CentralMailAnswer, central_mail::CentralMailResult,
};

#[tokio::test]
async fn worker_pushes_direct_mail_and_replacement_delivers_the_same_backlog_row() -> Result<()> {
    let fixture = Fixture::new().await?;
    let result = delivery_journey(&fixture).await;
    fixture.close().await?;
    result
}

async fn delivery_journey(fixture: &Fixture) -> Result<()> {
    let mut original = ConnectedLink::open(fixture).await?;
    let Caller::Session(session) = fixture::session_caller()? else {
        return Err("fixture must be a session".into());
    };
    let operations = [
        json!({
            "requestId":"019f0000-0000-7000-8000-00000000cf31",
            "operation":{"kind":"setNoticeCadence","cadence":"m15"},
        }),
        json!({
            "requestId":"019f0000-0000-7000-8000-00000000cf32",
            "operation":{
                "kind":"sendMessage","to":session,"body":"worker direct delivery","delivery":"queue",
                "replyTo":null,"sentAt":chrono::DateTime::from_timestamp(signed::NOW_SECONDS, 0).ok_or("fixture UTC")?
            },
        }),
    ];
    let requests = operations
        .into_iter()
        .map(|value| serde_json::from_value(value).map(RequestPayload::Mail))
        .collect::<std::result::Result<Vec<_>, _>>()?;
    let (_, replies) = original.batch(requests).await?;
    let [_, DataFrame::Reply { result, .. }] = replies.as_slice() else {
        return Err("expected both mail replies before delivery".into());
    };
    let ReplyPayload::Mail(CentralMailResult::Done(CentralMailAnswer::Queued { message_id })) =
        result.as_ref()
    else {
        return Err("the direct message was not accepted".into());
    };
    let offered = fixture::receive(&mut original.socket).await?;
    if !matches!(&offered, DataFrame::Deliver {
        message, sender: ResolvedSender::Session { session: sender, display_name: None, machine_label: Some(label) },
    } if &message.message_id == message_id && message.target == session && sender == &session && !label.as_str().is_empty())
    {
        return Err("the Worker did not push the accepted record with the resolved sender".into());
    }
    let wrong_recipient = "019f0000-0000-7000-8000-00000000cf0e";
    let other_token = signed::enroll_another_machine(
        &fixture.client,
        &fixture.host.signin_origin(),
        ORGANIZATION,
        wrong_recipient,
        "019f0000-0000-7000-8000-00000000e402",
        signed::NOW_SECONDS,
    )
    .await?;
    let mut wrong_machine = fixture
        .client
        .get(fixture.link_url(ORGANIZATION))
        .bearer_auth(other_token)
        .upgrade()
        .send()
        .await?
        .into_websocket()
        .await?;
    if !matches!(fixture::receive(&mut wrong_machine).await?, DataFrame::Welcome { machine_id, .. } if machine_id.as_str()==wrong_recipient)
    {
        return Err("the wrong-recipient probe needs an independently admitted machine".into());
    }
    let before_ack_log = fixture.host.captured_logs().len();
    fixture::send(
        &mut wrong_machine,
        &DataFrame::Ack {
            message_id: message_id.clone(),
            answer: central_link_protocol::DeliveryAnswer::Accepted,
        },
    )
    .await?;
    fixture
        .host
        .until_log_after(
            before_ack_log,
            "machine_delivery_ack_ignored reason=WrongMachine",
        )
        .await;
    wrong_machine.send(Message::Text("ping".to_owned())).await?;
    if !matches!(wrong_machine.next().await, Some(Ok(Message::Text(pong))) if pong=="pong") {
        return Err("a wrong-machine Ack must keep the socket open".into());
    }
    wrong_machine.close(CloseCode::Normal, None).await?;
    let mut replacement = ConnectedLink::open(fixture).await?;
    if fixture::receive(&mut replacement.socket).await? != offered {
        return Err("replacement did not receive the original unacknowledged record".into());
    }
    drop(original);
    fixture::send(
        &mut replacement.socket,
        &DataFrame::Ack {
            message_id: message_id.clone(),
            answer: central_link_protocol::DeliveryAnswer::Accepted,
        },
    )
    .await?;
    let status_limit = 1.try_into()?;
    let status_query = |message_id| WorkspaceRequest {
        request_id: None,
        workspace_id: None,
        operation: WorkspaceOperation::List {
            query: workspace_model::ListQuery::MessageStatus { message_id },
            cursor: workspace_model::ListCursor::start(workspace_model::ListOrder::OldestFirst),
            limit: status_limit,
        },
    };
    let status = replacement.call(status_query(message_id.clone())).await?;
    if !matches!(
        workspace_reply(&status.reply)?.result,
        WorkspaceResult::Done(WorkspaceAnswer::MessageStatus {
            status: workspace_model::central_mail::MessageStatus::Delivered {}
        })
    ) {
        return Err("the Ack did not settle the owning message row".into());
    }
    let another = serde_json::from_value(json!({
        "requestId":"019f0000-0000-7000-8000-00000000cf33",
        "operation":{
            "kind":"sendMessage","to":session,"body":"push after the old socket closes",
            "delivery":"queue","replyTo":null,"sentAt":chrono::DateTime::from_timestamp(signed::NOW_SECONDS, 0).ok_or("fixture UTC")?
        },
    }))?;
    let (_, replies) = replacement
        .batch(vec![RequestPayload::Mail(another)])
        .await?;
    let [DataFrame::Reply { result, .. }] = replies.as_slice() else {
        return Err("missing reply after the older socket closes".into());
    };
    let ReplyPayload::Mail(CentralMailResult::Done(CentralMailAnswer::Queued {
        message_id: next_message,
    })) = result.as_ref()
    else {
        return Err("the replacement's next send was not accepted".into());
    };
    if !matches!(fixture::receive(&mut replacement.socket).await?, DataFrame::Deliver { message, .. } if &message.message_id == next_message)
    {
        return Err("closing the previous socket lost the registered replacement".into());
    }
    let accepted = |message_id| DataFrame::Ack {
        message_id,
        answer: central_link_protocol::DeliveryAnswer::Accepted,
    };
    fixture::send(&mut replacement.socket, &accepted(next_message.clone())).await?;
    if !matches!(
        workspace_reply(
            &replacement
                .call(status_query(next_message.clone()))
                .await?
                .reply
        )?
        .result,
        WorkspaceResult::Done(WorkspaceAnswer::MessageStatus {
            status: workspace_model::central_mail::MessageStatus::Delivered {}
        })
    ) {
        return Err("second Ack must settle before reconnect".into());
    }
    let settled_rows = || Statement {
        sql: "SELECT message_id,message_kind,state,updated_at FROM mail_messages ORDER BY message_id",
        args: vec![],
    };
    let before = fixture.runtime().read(settled_rows()).await?;
    super::renewal::advance(fixture, 1).await?;
    let mut reconnected = ConnectedLink::open(fixture).await?;
    fixture::send(&mut reconnected.socket, &accepted(message_id.clone())).await?;
    if !matches!(
        workspace_reply(
            &reconnected
                .call(status_query(message_id.clone()))
                .await?
                .reply
        )?
        .result,
        WorkspaceResult::Done(WorkspaceAnswer::MessageStatus {
            status: workspace_model::central_mail::MessageStatus::Delivered {}
        })
    ) {
        return Err("the replayed Ack must remain silent and keep the socket open".into());
    }
    if fixture.runtime().read(settled_rows()).await? != before {
        return Err("the replayed Ack rewrote a settled delivery or added an outcome".into());
    }
    reconnected.close().await
}
