use crate::{object_context::ObjectContext, object_rpc, object_shell::ShellActor};
use collaboration_central_conformance::{ControlResult, StorageCorruption};
use collaboration_central_core::*;
use futures_channel::oneshot;
use serde::{Deserialize, Serialize};
use std::{cell::RefCell, collections::BTreeMap};
use worker::*;
#[derive(Default)]
pub(crate) struct SignalSlots {
    active: RefCell<BTreeMap<String, bool>>,
    waiting: RefCell<BTreeMap<String, Vec<oneshot::Sender<()>>>>,
}
impl SignalSlots {
    fn arm(&self, name: &str) {
        self.active.borrow_mut().insert(name.into(), false);
    }
    fn signal(&self, name: &str) {
        self.active.borrow_mut().insert(name.into(), true);
        for sender in self.waiting.borrow_mut().remove(name).into_iter().flatten() {
            let _ = sender.send(());
        }
    }
    async fn wait(&self, name: &str) -> Result<()> {
        if self.active.borrow().get(name) == Some(&true) {
            return Ok(());
        }
        let (sender, receiver) = oneshot::channel();
        self.waiting
            .borrow_mut()
            .entry(name.into())
            .or_default()
            .push(sender);
        receiver
            .await
            .map_err(|_| Error::RustError("test signal canceled".into()))
    }
}
#[derive(Serialize, Deserialize)]
#[serde(
    tag = "kind",
    content = "value",
    rename_all = "camelCase",
    deny_unknown_fields
)]
pub(crate) enum TestRequest {
    DirectoryUpdates,
    Register(ObjectAddress),
    Clock(RegistryTime),
    Known,
    Arm(String),
    Signal(String),
    Wait(String),
    Step { input: WireInput, now: RegistryTime },
    Full(u64),
    Panic(u64),
    Corrupt(StorageCorruption),
    Evict,
    Alarms,
    TakeHeldDispatch,
}
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct TestEnvelope {
    address: ObjectAddress,
    face: CallerKind,
    caller_kind: CallerKind,
    command: TestRequest,
}
pub(crate) async fn test<To: CoreKind>(
    env: &Env,
    to: To::Address,
    command: TestRequest,
) -> Result<Response>
where
    EdgeKind: MayCall<To>,
{
    let address = To::address(&to);
    object_rpc::request(
        env,
        &address,
        "/test/control",
        &TestEnvelope {
            address: address.clone(),
            face: To::KIND,
            caller_kind: EdgeKind::KIND,
            command,
        },
    )
    .await
}
pub(crate) async fn test_address(
    env: &Env,
    address: &ObjectAddress,
    command: TestRequest,
) -> Result<Response> {
    match address {
        ObjectAddress::Registry => test::<RegistryKind>(env, (), command).await,
        ObjectAddress::Fleet => test::<FleetKind>(env, (), command).await,
        ObjectAddress::Copy(project) => test::<CopyKind>(env, project.clone(), command).await,
        ObjectAddress::Connection(installation) => {
            test::<SessionKind>(env, installation.clone(), command).await
        }
    }
}
fn scoped(context: &ObjectContext, name: &str) -> String {
    format!("{:?}:{name}", context.face)
}
fn flag(context: &ObjectContext, name: &str) -> String {
    format!("_cf_test_flag_{}", scoped(context, name))
}
pub(crate) async fn consume(context: &ObjectContext, name: &str) -> Result<bool> {
    let key = flag(context, name);
    let enabled = context.storage.get::<bool>(&key).await?.unwrap_or(false);
    if enabled {
        context.storage.delete(&key).await?;
    }
    Ok(enabled)
}
pub(crate) fn signal(context: &ObjectContext, name: &str) {
    context.signals.signal(&scoped(context, name));
}
async fn wait(context: &ObjectContext, name: &str) -> Result<()> {
    context.signals.wait(&scoped(context, name)).await
}
pub(crate) async fn remember(
    context: &ObjectContext,
    address: ObjectAddress,
) -> Result<Option<RegistryTime>> {
    if context.address != ObjectAddress::Registry {
        return Err(Error::RustError("test inventory owner differs".into()));
    }
    let mut known = context
        .storage
        .get::<Vec<ObjectAddress>>("_cf_test_known")
        .await?
        .unwrap_or_default();
    if !known.contains(&address) {
        known.push(address);
        context.storage.put("_cf_test_known", known).await?;
    }
    context.storage.get("_cf_test_clock").await
}
pub(crate) async fn remember_outgoing<From: MayCall<To>, To: ObjectKind>(
    context: &ObjectContext,
    to: &To::Address,
) -> Result<()> {
    if From::KIND == CallerKind::Registry {
        remember(context, To::address(to)).await?;
    }
    Ok(())
}
pub(crate) async fn edge_clock(env: &Env, address: ObjectAddress) -> Result<Option<RegistryTime>> {
    object_rpc::decode(test::<RegistryKind>(env, (), TestRequest::Register(address)).await?).await
}
pub(crate) async fn before_dispatch(context: &ObjectContext, call: &WireCall) -> Result<()> {
    if matches!(call, WireCall::Copy(CopyCall::Fence { .. }))
        && consume(context, "hold_fence").await?
    {
        signal(context, "fence_held");
        wait(context, "resume_fence").await?;
    }
    if matches!(
        call,
        WireCall::Delivery(DeliveryCall::DeliverForward { .. })
    ) && consume(context, "hold_forward_delivery").await?
    {
        signal(context, "forward_delivery_held");
        wait(context, "resume_forward_delivery").await?;
    }
    if consume(context, "hold_dispatch").await? {
        context.storage.put("_cf_test_held_dispatch", call).await?;
        signal(context, "dispatch_held");
        wait(context, "resume_dispatch").await?;
    }
    Ok(())
}
pub(crate) async fn object_control(
    actor: &ShellActor,
    _path: &str,
    request: &mut Request,
) -> Result<Response> {
    let envelope: TestEnvelope = serde_json::from_str(&request.text().await?)?;
    if validate_call_edge(envelope.caller_kind, envelope.face).is_err()
        || envelope.face != envelope.address.primary_face()
    {
        return Response::error("test call edge refused", 403);
    }
    let context = actor.context(Some(envelope.address), envelope.face).await?;
    match envelope.command {
        TestRequest::DirectoryUpdates => Response::from_json(
            &context
                .storage
                .get::<Vec<DirectoryUpdate>>("_cf_test_directory_updates")
                .await?
                .unwrap_or_default(),
        ),
        TestRequest::Register(address) => Response::from_json(&remember(&context, address).await?),
        TestRequest::Known => {
            let mut known = context
                .storage
                .get::<Vec<ObjectAddress>>("_cf_test_known")
                .await?
                .unwrap_or_default();
            for address in [ObjectAddress::Registry, ObjectAddress::Fleet] {
                if !known.contains(&address) {
                    known.push(address);
                }
            }
            Response::from_json(&known)
        }
        TestRequest::Clock(now) => {
            context.storage.put("_cf_test_clock", now).await?;
            if let Some(at) = context.storage.get_alarm().await?
                && at <= now.0.timestamp_millis()
            {
                context.storage.delete_alarm().await?;
                for face in context.take_due_alarms().await? {
                    actor
                        .context(None, face)
                        .await?
                        .run(WireInput::Alarm { face })
                        .await?;
                }
            }
            context.refresh_alarm().await?;
            Response::from_json(&())
        }
        TestRequest::Arm(name) => {
            context.storage.put(&flag(&context, &name), true).await?;
            let (observed, resume) = match name.as_str() {
                "drop_answer" => ("answer_dropped", None),
                "hold_dispatch" => ("dispatch_held", Some("resume_dispatch")),
                "hold_fence" => ("fence_held", Some("resume_fence")),
                "hold_forward_delivery" => {
                    ("forward_delivery_held", Some("resume_forward_delivery"))
                }
                _ => ("unused", None),
            };
            actor.signals.arm(&scoped(&context, observed));
            if let Some(resume) = resume {
                actor.signals.arm(&scoped(&context, resume));
            }
            Response::from_json(&())
        }
        TestRequest::Signal(name) => {
            signal(&context, &name);
            Response::from_json(&())
        }
        TestRequest::Wait(name) => {
            wait(&context, &name).await?;
            Response::from_json(&())
        }
        TestRequest::Step { input, now } => {
            let face = input.face();
            let owned = actor.context(None, face).await?;
            let effects = owned.step_at(input, now).await?.into_trace();
            // Stepped RPC effects stay held; alarm scheduling uses the same
            // durable core plan as normal execution, including after restart.
            owned.refresh_alarm().await?;
            Response::from_json(&ControlResult::Effects(effects))
        }
        TestRequest::Full(writes) => {
            context.storage.put("_cf_test_full", writes).await?;
            Response::from_json(&())
        }
        TestRequest::Panic(writes) => {
            #[cfg(feature = "conformance-faults")]
            {
                context.storage.put("_cf_test_panic", writes).await?;
                Response::from_json(&())
            }
            #[cfg(not(feature = "conformance-faults"))]
            {
                let _ = writes;
                Response::error("panic faults not compiled", 404)
            }
        }
        TestRequest::Corrupt(mutation) => {
            crate::conformance_storage::corrupt(&context, mutation).await?;
            Response::from_json(&())
        }
        TestRequest::Evict => {
            drop(context);
            actor.runtime.borrow_mut().take();
            Response::from_json(&())
        }
        TestRequest::Alarms => Response::from_json(
            &context
                .storage
                .get_alarm()
                .await?
                .and_then(chrono::DateTime::from_timestamp_millis)
                .map(|time| vec![(context.address, RegistryTime(time))])
                .unwrap_or_default(),
        ),
        TestRequest::TakeHeldDispatch => {
            let held = context
                .storage
                .get::<WireCall>("_cf_test_held_dispatch")
                .await?;
            context.storage.delete("_cf_test_held_dispatch").await?;
            Response::from_json(&held)
        }
    }
}
