//! Sender controls run through admitted sockets and the real recipient mailbox.
use super::relay_transport as relay;
use board_sync_protocol::{
    BoardConsumerAnswer, BoardConsumerProtocol, BoardDeliveryTypes, MailSubmitAnswer,
};
use collaboration_central_conformance::ConformanceResult;
use collaboration_central_conformance::{ConformanceClient, enroll_admitted_peer};
use durable_steps::Uuid;
use fleet_sync_protocol::*;
use reqwest_websocket::WebSocket;
use serde_json::json;

fn item(peer: PeerId, service: &str) -> ConformanceResult<DeliveryEnvelope<BoardDeliveryTypes>> {
    Ok(DeliveryEnvelope::Item(
        DeliveryItem::<BoardDeliveryTypes>::new(
            ItemId::new(Uuid::now_v7())?,
            serde_json::from_value(
                json!({"kind":"humanUser","installation":"550e8400-e29b-41d4-a716-446655440525"}),
            )?,
            serde_json::from_value(
                json!({"peer":peer,"address":{"endpoint":{"serviceId":service,"endpointId":"codex-local"},"sessionId":"sender-controls-recipient"}}),
            )?,
            None,
            serde_json::from_value(
                json!({"kind":"message","text":"sender controls immutable custody","delivery":"auto","guard":null,"senderDisplayName":null,"replyTo":null}),
            )?,
        )?,
    ))
}
async fn request(
    socket: &mut WebSocket,
    kind: &str,
    value: serde_json::Value,
) -> ConformanceResult<OperationAnswer<BoardConsumerProtocol>> {
    Ok(serde_json::from_value(
        relay::operation(socket, relay::relay_request(kind, value)).await?,
    )?)
}
async fn submit(
    socket: &mut WebSocket,
    item: &DeliveryEnvelope<BoardDeliveryTypes>,
) -> ConformanceResult<OperationAnswer<BoardConsumerProtocol>> {
    Ok(serde_json::from_value(relay::operation(socket,json!({"kind":"consumer","value":{"kind":"mailSubmit","value":{"item":item,"originEpoch":null}}})).await?)?)
}

pub(super) async fn raw_submit_is_refused_without_custody_and_the_same_socket_remains_usable(
    base_url: &str,
) -> ConformanceResult<()> {
    async {
        let client=ConformanceClient::new(base_url)?;
        let mut sender=enroll_admitted_peer(&client,"550e8400-e29b-41d4-a716-446655440525","019f0000-0000-7000-8000-000000000525","raw-submit-origin","019f0000-0000-7000-8000-000000005251").await?;
        let mut recipient=enroll_admitted_peer(&client,"550e8400-e29b-41d4-a716-446655440526","019f0000-0000-7000-8000-000000000526","raw-submit-recipient","019f0000-0000-7000-8000-000000005252").await?;
        let proof=async {
            let item=item(PeerId::new(recipient.peer)?,"550e8400-e29b-41d4-a716-446655440526")?;
            if !matches!(request(&mut sender.socket,"submit",json!({"item":item})).await?,OperationAnswer::InvalidShape {field} if field=="operation") {return Err("raw Submit bypassed the board provenance gate".into());}
            if !matches!(request(&mut sender.socket,"statusFetch",json!({"after":0,"limit":10})).await?,OperationAnswer::Delivery(DeliveryAnswer::StatusFetch(_))) {return Err("raw Submit refusal damaged its admitted socket".into());}
            let OperationAnswer::Delivery(DeliveryAnswer::Fetch(fetched))=request(&mut recipient.socket,"fetch",json!({"stream":"items","after":0,"maxItems":10,"maxBytes":1_000_000})).await? else {return Err("recipient Fetch lost its typed answer".into());};
            if !fetched.entries.is_empty() || fetched.through.value()!=0 {return Err("raw Submit created recipient custody".into());}
            Ok(())
        }.await;
        let _=sender.socket.close(reqwest_websocket::CloseCode::Normal,None).await;let _=recipient.socket.close(reqwest_websocket::CloseCode::Normal,None).await;proof
    }.await
}
pub(super) async fn sender_query_and_settlement_preserve_existing_custody_and_pin_unknown_identity(
    base_url: &str,
) -> ConformanceResult<()> {
    async {
        let client=ConformanceClient::new(base_url)?;
        let mut sender=enroll_admitted_peer(&client,"550e8400-e29b-41d4-a716-446655440525","019f0000-0000-7000-8000-000000000525","settlement-origin","019f0000-0000-7000-8000-000000005253").await?;
        let mut recipient=enroll_admitted_peer(&client,"550e8400-e29b-41d4-a716-446655440526","019f0000-0000-7000-8000-000000000526","settlement-recipient","019f0000-0000-7000-8000-000000005254").await?;
        let proof=async {
            let mailbox=PeerId::new(recipient.peer)?;
            let existing=item(mailbox,"550e8400-e29b-41d4-a716-446655440526")?;
            let unknown=item(mailbox,"550e8400-e29b-41d4-a716-446655440526")?;
            if !matches!(submit(&mut sender.socket,&existing).await?,OperationAnswer::Consumer(BoardConsumerAnswer::MailSubmit(MailSubmitAnswer::Submitted(SubmitResult::Accepted {..})))) {return Err("valid board submit did not establish real custody".into());}
            let OperationAnswer::Delivery(DeliveryAnswer::Query(before))=request(&mut sender.socket,"query",json!({"mailbox":mailbox,"items":[existing.id(),unknown.id()]})).await? else {return Err("sender Query did not reach the recipient mailbox".into());};
            if before.statuses.len()!=2 || !before.statuses.iter().any(|status|status.item==unknown.id() && matches!(status.state,ItemStatusState::Unknown)) {return Err("Query fabricated a terminal for unknown custody".into());}
            if !matches!(request(&mut sender.socket,"settle",json!({"mailbox":mailbox,"item":existing.id(),"digest":existing.digest()})).await?,OperationAnswer::Delivery(DeliveryAnswer::Settle(SettleResult::Replayed {status:DeliveryStatus::Sent {..},..}))) {return Err("Settle replaced known custody with a tombstone".into());}
            if !matches!(request(&mut sender.socket,"settle",json!({"mailbox":mailbox,"item":unknown.id(),"digest":unknown.digest()})).await?,OperationAnswer::Delivery(DeliveryAnswer::Settle(SettleResult::Settled {reason:DeliveryFailureKind::CustodyUnresolvable,..}))) {return Err("Settle did not establish the relay-owned unknown outcome".into());}
            if !matches!(submit(&mut sender.socket,&unknown).await?,OperationAnswer::Consumer(BoardConsumerAnswer::MailSubmit(MailSubmitAnswer::Submitted(SubmitResult::Replayed {status:DeliveryStatus::Refused {reason:DeliveryRefusal::ProcessingFailed {kind:DeliveryFailureKind::CustodyUnresolvable}},..})))) {return Err("late Submit reopened a settled unknown identity".into());}
            let OperationAnswer::Delivery(DeliveryAnswer::Fetch(fetched))=request(&mut recipient.socket,"fetch",json!({"stream":"items","after":0,"maxItems":10,"maxBytes":1_000_000})).await? else {return Err("recipient fetch lost custody readback".into());};
            if fetched.entries.len()!=1 || !matches!(fetched.entries.first(),Some(FetchEntry::Item {item,..}) if item.id()==existing.id()) {return Err("settlement altered existing custody or created a delivery for the tombstone".into());}
            Ok(())
        }.await;
        let _=sender.socket.close(reqwest_websocket::CloseCode::Normal,None).await;let _=recipient.socket.close(reqwest_websocket::CloseCode::Normal,None).await;proof
    }.await
}
pub(super) async fn retirement_uses_the_verified_sender_and_replays_its_original_call_receipt(
    base_url: &str,
) -> ConformanceResult<()> {
    async {
        let client = ConformanceClient::new(base_url)?;
        let mut sender = enroll_admitted_peer(&client,"550e8400-e29b-41d4-a716-446655440525","019f0000-0000-7000-8000-000000000525","retirement-origin","019f0000-0000-7000-8000-000000005255").await?;
        let mut recipient = enroll_admitted_peer(&client,"550e8400-e29b-41d4-a716-446655440526","019f0000-0000-7000-8000-000000000526","retirement-recipient","019f0000-0000-7000-8000-000000005256").await?;
        let proof = async {
            let mailbox = PeerId::new(recipient.peer)?;
            let envelope = item(mailbox,"550e8400-e29b-41d4-a716-446655440526")?;
            assert!(matches!(submit(&mut sender.socket,&envelope).await?,
                OperationAnswer::Consumer(BoardConsumerAnswer::MailSubmit(MailSubmitAnswer::Submitted(SubmitResult::Accepted {..})))));
            let report = request(&mut recipient.socket,"report",json!({"report":{"item":envelope.id(),"reportSeq":1,"outcome":{"kind":"dropped"}}})).await?;
            assert!(matches!(report,OperationAnswer::Delivery(DeliveryAnswer::Report(_))));

            let OperationAnswer::Delivery(DeliveryAnswer::Retire(refused)) = request(&mut recipient.socket,"retire",json!({"mailbox":mailbox,"items":[envelope.id()]})).await? else {
                return Err("Retire did not reach its typed mailbox owner".into());
            };
            assert_eq!(refused.not_current_target,vec![envelope.id()]);
            assert!(refused.retired.is_empty());
            let retire = relay::relay_request("retire",json!({"mailbox":mailbox,"items":[envelope.id()]}));
            let first = relay::operation(&mut sender.socket,retire.clone()).await?;
            assert_eq!(first,json!({"kind":"delivery","value":{"kind":"retire","value":{"retired":[envelope.id()],"unknown":[],"notCurrentTarget":[]}}}));
            assert_eq!(relay::operation(&mut sender.socket,retire).await?, first);
            let OperationAnswer::Delivery(DeliveryAnswer::Fetch(controls)) = request(&mut recipient.socket,"fetch",json!({"stream":"controls","after":0,"maxItems":10,"maxBytes":1_000_000})).await? else {
                return Err("recipient control readback lost its typed answer".into());
            };
            assert_eq!(controls.entries.len(),1,"Retire replay created another control effect");
            assert!(matches!(controls.entries.first(),Some(FetchEntry::Retired {items,..}) if items==&vec![envelope.id()]));
            Ok(())
        }.await;
        let _=sender.socket.close(reqwest_websocket::CloseCode::Normal,None).await;
        let _=recipient.socket.close(reqwest_websocket::CloseCode::Normal,None).await;
        proof
    }.await
}
