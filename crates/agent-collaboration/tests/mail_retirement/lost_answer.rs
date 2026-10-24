//! A lost post-commit answer keeps the same durable retirement call across restart.
use crate::{
    central_pair::{self, SecondHost},
    fixture::{self, BoardFixture},
    readback, wake_and_capacity,
};
use board_central_test_support::{DispatchFaultObservation, ObjectAddress, RuntimeControl};
use codex_router_test_support::FixtureResult;
use collaboration_client::protocol::EndpointRef;
use serde_json::json;

pub async fn run(fixture: &mut BoardFixture, sender_host: &mut SecondHost) -> FixtureResult {
    let service: String = fixture
        .installation
        .as_ref()
        .ok_or("recipient absent")?
        .service_id
        .clone()
        .into();
    let endpoint: EndpointRef =
        serde_json::from_value(json!({"serviceId":service,"endpointId":"claude-local"}))?;
    let mut peer = wake_and_capacity::start_peer(fixture, &endpoint).await?;
    // Sender Submit/Retire resolve the enrolled peer through Registry, then call Mailbox.
    let origin = ObjectAddress::Registry;
    let destination = ObjectAddress::Mailbox(service.clone().try_into()?);
    let central = central_pair::central(fixture)?;
    let result=async {
        central.runtime_control(origin.clone(),RuntimeControl::HoldNextDispatch{destination:destination.clone()}).await?;
        let sender=wake_and_capacity::sender(sender_host)?;
        let item=fixture::identity();
        wake_and_capacity::call(sender_host.host()?,"message_send",json!({"itemId":item,"target":peer.target,
            "message":{"kind":"agent","sender":sender,"text":"lost retirement answer marker"},"delivery":"auto"})).await?;
        central.runtime_control(origin.clone(),RuntimeControl::AwaitHeldDispatch).await.map_err(|error|format!("initial submit gate: {error}"))?;
        central.runtime_control(origin.clone(),RuntimeControl::ResumeDispatchAndHoldNext{destination:destination.clone()}).await?;
        peer.expect_marker("lost retirement answer marker").await?;
        let held=central.runtime_control(origin.clone(),RuntimeControl::AwaitHeldDispatch).await.map_err(|error|format!("initial retirement gate: {error}"))?;
        let DispatchFaultObservation::Held{request,..}=held.dispatch else {return Err("retirement dispatch was not held".into())};
        assert!(std::str::from_utf8(&request)?.contains("retire"),"held request must be the actual origin retirement");
        central.runtime_control(origin.clone(),RuntimeControl::DropNextObjectReply{destination:destination.clone()}).await?;
        central.runtime_control(origin.clone(),RuntimeControl::ResumeDispatchAndHoldNext{destination:destination.clone()}).await?;
        central.runtime_control(origin.clone(),RuntimeControl::AwaitDroppedObjectReply).await.map_err(|error|format!("lost retirement reply: {error}"))?;
        readback::require_compacted(fixture,&service,&item).await?;
        let retried=central.runtime_control(origin.clone(),RuntimeControl::AwaitHeldDispatch).await.map_err(|error|format!("retirement retry gate: {error}"))?;
        let DispatchFaultObservation::Held{request:retried,..}=retried.dispatch else {return Err("retirement retry was not held".into())};
        assert_eq!(retried,request,"lost answer changed the retirement call identity");
        sender_host.stop_host().await?;
        central.runtime_control(origin.clone(),RuntimeControl::ResumeDispatchAndHoldNext{destination}).await?;
        sender_host.reopen_host().await?;
        let resumed=central.runtime_control(origin.clone(),RuntimeControl::AwaitHeldDispatch).await.map_err(|error|format!("restart retirement gate: {error}"))?;
        let DispatchFaultObservation::Held{request:resumed,..}=resumed.dispatch else {return Err("restart did not resume retirement".into())};
        assert_eq!(resumed,request,"restart changed the durable retirement call identity");
        central.runtime_control(origin.clone(),RuntimeControl::ResumeDispatch).await?;
        assert_eq!(readback::terminal(sender_host.host()?,&sender,&item).await?.get("kind"),Some(&json!("delivered")));
        assert_eq!(peer.count_matching_after_barrier("lost retirement answer marker").await?,1);
        readback::require_compacted(fixture,&service,&item).await?;
        sender_host.stop_host().await?;
        readback::require_cold_terminal(&sender_host.directory,&item).await
    }.await;
    // Release only this fixture's held object call, including on the pre-fix failure.
    let _ = central
        .runtime_control(origin, RuntimeControl::ResumeDispatch)
        .await;
    result.and(peer.shutdown().await)
}
