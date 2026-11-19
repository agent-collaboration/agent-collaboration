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
    let status = replacement
        .call(WorkspaceRequest {
            request_id: None,
            workspace_id: None,
            operation: WorkspaceOperation::List {
                query: workspace_model::ListQuery::MessageStatus {
                    message_id: message_id.clone(),
                },
                cursor: workspace_model::ListCursor::start(workspace_model::ListOrder::OldestFirst),
                limit: 1.try_into()?,
            },
        })
        .await?;
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
    let ReplyPayload::Mail(CentralMailResult::Done(CentralMailAnswer::Queued { message_id })) =
        result.as_ref()
    else {
        return Err("the replacement's next send was not accepted".into());
    };
    if !matches!(fixture::receive(&mut replacement.socket).await?, DataFrame::Deliver { message, .. } if &message.message_id == message_id)
    {
        return Err("closing the previous socket lost the registered replacement".into());
    }
    replacement.close().await
}
