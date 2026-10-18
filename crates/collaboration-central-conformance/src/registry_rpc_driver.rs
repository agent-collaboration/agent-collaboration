//! Stage existing Registry RPC boundaries against the real platform storage.
use crate::{ConformanceClient, ConformanceResult, ControlRequest, ControlResult};
use collaboration_central_core::*;

#[derive(Clone)]
pub(crate) struct PendingRegistryRpc {
    pub call_id: ObjectCallId,
    pub address: ObjectAddress,
    pub call: WireCall,
}

pub(crate) fn dispatch(effects: &[WireEffect]) -> ConformanceResult<PendingRegistryRpc> {
    let calls: Vec<_> = effects
        .iter()
        .filter_map(|effect| match effect {
            WireEffect::Dispatch {
                call_id,
                to,
                call,
                carry: None,
            } => Some(PendingRegistryRpc {
                call_id: call_id.clone(),
                address: to.clone(),
                call: call.clone(),
            }),
            _ => None,
        })
        .collect();
    if calls.len() != 1 {
        return Err(format!("expected one Registry RPC, got {effects:?}").into());
    }
    calls
        .into_iter()
        .next()
        .ok_or_else(|| "Registry RPC".into())
}

pub(crate) async fn step(
    client: &ConformanceClient,
    input: WireInput,
    now: RegistryTime,
) -> ConformanceResult<Vec<WireEffect>> {
    let ControlResult::Effects(effects) = client
        .control(ControlRequest::Step {
            address: ObjectAddress::Registry,
            input: Box::new(input),
            now,
        })
        .await?
    else {
        return Err("Registry step effects".into());
    };
    Ok(effects)
}

pub(crate) async fn begin(
    client: &ConformanceClient,
    call: RegistryCall,
    now: RegistryTime,
) -> ConformanceResult<Vec<WireEffect>> {
    step(
        client,
        WireInput::Call {
            from: ObjectAddress::Registry,
            call_id: ObjectCallId::new(uuid::Uuid::now_v7().to_string())?,
            call: Box::new(WireCall::Registry(call)),
        },
        now,
    )
    .await
}

impl PendingRegistryRpc {
    pub async fn answer(&self, client: &ConformanceClient) -> ConformanceResult<WireResult> {
        client.call(self.address.clone(), self.call.clone()).await
    }
    pub async fn deliver(
        &self,
        client: &ConformanceClient,
        result: WireResult,
        now: RegistryTime,
    ) -> ConformanceResult<Vec<WireEffect>> {
        step(
            client,
            WireInput::CallResult {
                source: CallerKind::Registry,
                call_id: self.call_id.clone(),
                result: Box::new(result),
                carry: None,
            },
            now,
        )
        .await
    }
    pub async fn complete(
        &self,
        client: &ConformanceClient,
        now: RegistryTime,
    ) -> ConformanceResult<Vec<WireEffect>> {
        self.deliver(client, self.answer(client).await?, now).await
    }
}

pub(crate) fn future_time() -> ConformanceResult<RegistryTime> {
    let future = chrono::Utc::now() + chrono::TimeDelta::minutes(2);
    Ok(RegistryTime(
        chrono::DateTime::from_timestamp_millis(future.timestamp_millis()).ok_or("fixture time")?,
    ))
}
