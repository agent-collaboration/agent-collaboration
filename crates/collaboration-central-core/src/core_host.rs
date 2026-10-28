//! One shared-task Host; runtimes supply byte transport, time, IDs and wakes.

use crate::registry_peer_lookup::PeerDirectory;
use crate::{
    CallerContext, CopyKind, DeliveryKind, FleetKind, MailboxKind, ObjectAddress, RegistryKind,
    SerializedObjectCall, SessionKind,
    object_call_envelope::{encode_refusal, encode_shared_call},
};
use durable_steps::{CallFailure, CallId, CallTarget, Host, NotSentReason};
use fleet_sync_lease::{
    ClaimReservation, ClaimSettlement, CopyAcknowledged, CopyCreate, CopyFence, CopyRead,
    CopyRetire, HolderRelease, HolderWithdrawal, RegistrationAdmission, RetirementAdmission,
};
use fleet_sync_protocol::{DatabaseId, PeerId, ReceiverId};
use message_board::ServiceId;
use runtime_primitives::{Clock, IdSource, SyncTime, WakeFailure, WakeScheduler};
use serde::de::DeserializeOwned;
use std::future::Future;

mod copy_followup;
mod lease_publication;
mod lease_settlement;
mod registration_gate;
mod session_routes;

pub trait PlatformIo: Clock + IdSource + WakeScheduler + Send + Sync + 'static {
    fn object_call(
        &self,
        destination: ObjectAddress,
        body: Vec<u8>,
    ) -> impl Future<Output = Result<Vec<u8>, CallFailure>> + Send;
}

pub struct CoreHost<Io: PlatformIo> {
    io: std::sync::Arc<Io>,
    caller: SharedTaskCaller,
    publication: Option<crate::mail_status_dispatch::PublicationDispatcher>,
    authoritative_receiver: ReceiverId,
    #[cfg(feature = "conformance")]
    dispatches: std::sync::Arc<std::sync::Mutex<Vec<ObjectAddress>>>,
    #[cfg(feature = "conformance")]
    pub(crate) forward_route_probe: std::sync::Arc<crate::forward_observer::ForwardRouteProbe>,
    #[cfg(feature = "conformance")]
    pub(crate) revocation_probe:
        std::sync::Arc<crate::machine_enrollment::conformance_revocation::RevocationProbe>,
}

enum SharedTaskCaller {
    Registry {
        caller: CallerContext<RegistryKind>,
        peer_directory: PeerDirectory,
        lease_publication: Option<lease_settlement::RegistryLeasePublication>,
    },
    Copy {
        caller: CallerContext<CopyKind>,
        database: DatabaseId,
    },
    StoreOnly,
    Fleet(CallerContext<FleetKind>),
    Mailbox(CallerContext<MailboxKind>),
    Session(CallerContext<SessionKind>),
    Delivery,
}

impl<Io: PlatformIo> CoreHost<Io> {
    #[cfg(feature = "conformance")]
    pub fn conformance_registry(io: Io, authoritative_receiver: ReceiverId) -> Self {
        Self::registry(
            io,
            authoritative_receiver,
            std::sync::Arc::new(|_peer| Box::pin(std::future::ready(Err(no_route())))),
            None,
        )
    }

    pub(crate) fn registry(
        io: Io,
        authoritative_receiver: ReceiverId,
        peer_directory: PeerDirectory,
        lease_store: Option<std::sync::Arc<dyn crate::registry_lease_store::RegistryLeaseStore>>,
    ) -> Self {
        Self {
            io: std::sync::Arc::new(io),
            caller: SharedTaskCaller::Registry {
                caller: CallerContext::for_object(()),
                peer_directory,
                lease_publication: lease_store.map(lease_settlement::RegistryLeasePublication::new),
            },
            authoritative_receiver,
            publication: None,
            #[cfg(feature = "conformance")]
            dispatches: Default::default(),
            #[cfg(feature = "conformance")]
            forward_route_probe: Default::default(),
            #[cfg(feature = "conformance")]
            revocation_probe: Default::default(),
        }
    }

    pub(crate) fn fleet(io: std::sync::Arc<Io>, authoritative_receiver: ReceiverId) -> Self {
        Self {
            io,
            caller: SharedTaskCaller::Fleet(CallerContext::for_object(())),
            authoritative_receiver,
            publication: None,
            #[cfg(feature = "conformance")]
            dispatches: Default::default(),
            #[cfg(feature = "conformance")]
            forward_route_probe: Default::default(),
            #[cfg(feature = "conformance")]
            revocation_probe: Default::default(),
        }
    }

    pub(crate) fn authoritative_receiver(&self) -> ReceiverId {
        self.authoritative_receiver
    }

    pub fn for_store(io: Io, authoritative_receiver: ReceiverId) -> Self {
        Self {
            io: std::sync::Arc::new(io),
            caller: SharedTaskCaller::StoreOnly,
            authoritative_receiver,
            publication: None,
            #[cfg(feature = "conformance")]
            dispatches: Default::default(),
            #[cfg(feature = "conformance")]
            forward_route_probe: Default::default(),
            #[cfg(feature = "conformance")]
            revocation_probe: Default::default(),
        }
    }

    pub(crate) fn delivery(&self) -> Self {
        Self {
            io: self.io.clone(),
            caller: SharedTaskCaller::Delivery,
            authoritative_receiver: self.authoritative_receiver,
            publication: self.publication.clone(),
            #[cfg(feature = "conformance")]
            dispatches: self.dispatches.clone(),
            #[cfg(feature = "conformance")]
            forward_route_probe: self.forward_route_probe.clone(),
            #[cfg(feature = "conformance")]
            revocation_probe: self.revocation_probe.clone(),
        }
    }

    pub(crate) fn session(
        io: std::sync::Arc<Io>,
        authoritative_receiver: ReceiverId,
        installation: ServiceId,
        publication: crate::mail_status_dispatch::PublicationDispatcher,
    ) -> Self {
        Self {
            io,
            caller: SharedTaskCaller::Session(CallerContext::for_object(installation)),
            authoritative_receiver,
            publication: Some(publication),
            #[cfg(feature = "conformance")]
            dispatches: Default::default(),
            #[cfg(feature = "conformance")]
            forward_route_probe: Default::default(),
            #[cfg(feature = "conformance")]
            revocation_probe: Default::default(),
        }
    }

    pub(crate) fn mailbox(io: Io, receiver: ReceiverId, installation: ServiceId) -> Self {
        let mut host = Self::for_store(io, receiver);
        host.caller = SharedTaskCaller::Mailbox(CallerContext::for_object(installation));
        host
    }

    pub(crate) async fn gather_welcome(
        &self,
        peer: PeerId,
    ) -> Result<Vec<fleet_sync_protocol::DatabaseStatus>, CallFailure> {
        use fleet_sync_protocol::{CopyStatusAnswer, CopyUnavailableReason, DatabaseStatus};
        let leases = self
            .dispatch(
                crate::session_calls::WelcomeLeases,
                peer,
                self.call_identity("welcome_read")?,
            )
            .await?;
        let mut databases = Vec::new();
        for lease in leases {
            let database = lease.database;
            let status = self
                .dispatch(
                    CopyRead {
                        copy: self.authoritative_receiver,
                        database,
                    },
                    (),
                    self.call_identity("welcome_read")?,
                )
                .await;
            databases.push(match status {
                Ok(CopyStatusAnswer::Status(copy)) => DatabaseStatus::Ready {
                    database,
                    acked: copy.acked,
                    lease,
                },
                Ok(CopyStatusAnswer::Retired) => continue,
                Ok(CopyStatusAnswer::NotInitialized) => DatabaseStatus::Unavailable {
                    database,
                    lease: Some(lease),
                    reason: CopyUnavailableReason::Uninitialized,
                },
                Err(failure) => DatabaseStatus::Unavailable {
                    database,
                    lease: Some(lease),
                    reason: crate::copy_call_failure::unavailable(&failure).unwrap_or_else(|| {
                        CopyUnavailableReason::Damaged {
                            detail: "copy status unavailable".into(),
                        }
                    }),
                },
            });
        }
        Ok(databases)
    }

    #[cfg(feature = "conformance")]
    pub(crate) fn dispatched_destinations(
        &self,
    ) -> Result<Vec<ObjectAddress>, durable_steps::StoreError> {
        Ok(self
            .dispatches
            .lock()
            .map_err(durable_steps::StoreError::backend)?
            .clone())
    }

    pub(crate) fn call_identity(&self, step: &str) -> Result<CallId, CallFailure> {
        Ok(CallId {
            operation: durable_steps::OperationId::new(self.new_id()).map_err(encode_refusal)?,
            step: durable_steps::StepName::new(step).map_err(encode_refusal)?,
        })
    }

    #[cfg(feature = "conformance")]
    pub(crate) async fn replay_lifecycle_call(&self, identity: &CallId) -> Result<(), CallFailure> {
        let call = self
            .revocation_probe
            .recorded(identity)
            .map_err(|_| no_route())?;
        let answer = self.io.object_call(call.destination, call.bytes).await?;
        serde_json::from_slice(&answer).map_err(|_| CallFailure::AnswerLost)
    }

    async fn classify<Call: CallTarget>(
        &self,
        target: Call,
        request: Call::Request,
        identity: CallId,
    ) -> Result<SerializedObjectCall, CallFailure> {
        let target = serde_json::to_value(target).map_err(encode_refusal)?;
        let request = serde_json::to_value(request).map_err(encode_refusal)?;
        match &self.caller {
            SharedTaskCaller::Fleet(caller) => {
                use crate::machine_notice_delivery::DeliverMachineNotice;
                if Call::KIND != DeliverMachineNotice::KIND {
                    return Err(no_route());
                }
                let target: DeliverMachineNotice = decode(target)?;
                encode_shared_call::<FleetKind, DeliveryKind, DeliverMachineNotice>(
                    caller,
                    target.installation.clone(),
                    target,
                    decode(request)?,
                    identity,
                )
            }
            SharedTaskCaller::Mailbox(caller) => {
                if Call::KIND == crate::machine_notice_delivery::DeliverMachineNotice::KIND {
                    let target: crate::machine_notice_delivery::DeliverMachineNotice =
                        decode(target)?;
                    let notice: board_sync_protocol::BoardNotice = decode(request)?;
                    if caller.address() != ObjectAddress::Mailbox(target.installation.clone())
                        || !matches!(notice, board_sync_protocol::BoardNotice::MailWaiting { .. })
                    {
                        return Err(no_route());
                    }
                    return encode_shared_call::<
                        MailboxKind,
                        DeliveryKind,
                        crate::machine_notice_delivery::DeliverMachineNotice,
                    >(
                        caller,
                        target.installation.clone(),
                        target,
                        notice,
                        identity,
                    );
                }
                if Call::KIND != fleet_sync_delivery::PublishCall::<board_sync_protocol::BoardDeliveryTypes>::KIND {
                    return Err(no_route());
                }
                let target: fleet_sync_delivery::PublishCall<
                    board_sync_protocol::BoardDeliveryTypes,
                > = decode(target)?;
                let fleet_sync_delivery::PublicationTarget::StatusStream { route, .. } =
                    &target.target
                else {
                    return Err(no_route());
                };
                encode_shared_call::<
                    MailboxKind,
                    DeliveryKind,
                    fleet_sync_delivery::PublishCall<board_sync_protocol::BoardDeliveryTypes>,
                >(caller, route.clone(), target, decode(request)?, identity)
            }
            SharedTaskCaller::Session(caller) => session_routes::classify_session(
                Call::KIND,
                caller,
                self.authoritative_receiver,
                target,
                request,
                identity,
            ),
            SharedTaskCaller::StoreOnly
            | SharedTaskCaller::Delivery
            | SharedTaskCaller::Copy { .. } => Err(no_route()),
            SharedTaskCaller::Registry {
                caller,
                peer_directory,
                ..
            } => {
                macro_rules! to_copy {
                    ($target:ty, $database:expr) => {{
                        let target: $target = decode(target)?;
                        if target.copy != self.authoritative_receiver {
                            return Err(no_route());
                        }
                        let request: <$target as CallTarget>::Request = decode(request)?;
                        let database: DatabaseId = ($database)(&target, &request);
                        encode_shared_call::<RegistryKind, CopyKind, $target>(
                            caller, database, target, request, identity,
                        )
                    }};
                }
                macro_rules! to_fleet {
                    ($target:ty) => {
                        encode_shared_call::<RegistryKind, FleetKind, $target>(
                            caller,
                            (),
                            decode(target)?,
                            decode(request)?,
                            identity,
                        )
                    };
                }
                match Call::KIND {
                    crate::machine_notice_delivery::DeliverMachineNotice::KIND => {
                        let target: crate::machine_notice_delivery::DeliverMachineNotice = decode(target)?;
                        encode_shared_call::<RegistryKind, DeliveryKind, crate::machine_notice_delivery::DeliverMachineNotice>(
                            caller, target.installation.clone(), target, decode(request)?, identity,
                        )
                    }
                    crate::mail_submit_calls::MailboxSubmit::KIND => {
                        let target: crate::mail_submit_calls::MailboxSubmit = decode(target)?;
                        encode_shared_call::<RegistryKind, MailboxKind, crate::mail_submit_calls::MailboxSubmit>(
                            caller, target.installation.clone(), target, decode(request)?, identity,
                        )
                    }
                    crate::mailbox_requests::MailboxRequest::KIND => {
                        let target: crate::mailbox_requests::MailboxRequest = decode(target)?;
                        encode_shared_call::<RegistryKind, MailboxKind, crate::mailbox_requests::MailboxRequest>(
                            caller, target.installation.clone(), target, decode(request)?, identity,
                        )
                    }
                    #[cfg(feature = "conformance")]
                    crate::conformance_copy_wait::CopyWaitTarget::KIND => to_copy!(
                        crate::conformance_copy_wait::CopyWaitTarget,
                        |target: &crate::conformance_copy_wait::CopyWaitTarget, _: &crate::conformance_copy_wait::CopyWaitControl| target.database
                    ),
                    CopyRead::KIND => {
                        to_copy!(CopyRead, |target: &CopyRead, _: &()| target.database)
                    }
                    CopyAcknowledged::KIND => to_copy!(
                        CopyAcknowledged,
                        |target: &CopyAcknowledged, _: &fleet_sync_protocol::AwaitAcked| target
                            .database
                    ),
                    CopyFence::KIND => to_copy!(
                        CopyFence,
                        |_: &CopyFence, request: &fleet_sync_protocol::FenceRequest| request
                            .database
                    ),
                    CopyCreate::KIND => to_copy!(
                        CopyCreate,
                        |_: &CopyCreate, request: &fleet_sync_protocol::CreateCopy| request
                            .database
                    ),
                    CopyRetire::KIND => to_copy!(
                        CopyRetire,
                        |_: &CopyRetire, request: &fleet_sync_protocol::RetireCopy| request
                            .database
                    ),
                    crate::machine_enrollment::EndPeerSessions::KIND => {
                        let target: crate::machine_enrollment::EndPeerSessions = decode(target)?;
                        encode_shared_call::<RegistryKind, DeliveryKind, crate::machine_enrollment::EndPeerSessions>(
                            caller,
                            peer_directory(target.peer).await?,
                            target,
                            decode(request)?,
                            identity,
                        )
                    }
                    crate::mailbox_lifecycle::ClosePeerMailbox::KIND => {
                        let target: crate::mailbox_lifecycle::ClosePeerMailbox = decode(target)?;
                        encode_shared_call::<RegistryKind, MailboxKind, crate::mailbox_lifecycle::ClosePeerMailbox>(
                            caller,
                            peer_directory(target.peer).await?,
                            target,
                            decode(request)?,
                            identity,
                        )
                    }
                    crate::mailbox_lifecycle::OpenPeerMailbox::KIND => {
                        let target: crate::mailbox_lifecycle::OpenPeerMailbox = decode(target)?;
                        encode_shared_call::<RegistryKind, MailboxKind, crate::mailbox_lifecycle::OpenPeerMailbox>(
                            caller,
                            peer_directory(target.peer).await?,
                            target,
                            decode(request)?,
                            identity,
                        )
                    }
                    HolderRelease::KIND => {
                        let target: HolderRelease = decode(target)?;
                        let request: fleet_sync_protocol::ReleaseRequest = decode(request)?;
                        if target.database != request.database {
                            return Err(no_route());
                        }
                        encode_shared_call::<RegistryKind, DeliveryKind, HolderRelease>(
                            caller,
                            peer_directory(target.holder).await?,
                            target,
                            request,
                            identity,
                        )
                    }
                    HolderWithdrawal::KIND => {
                        let target: HolderWithdrawal = decode(target)?;
                        encode_shared_call::<RegistryKind, DeliveryKind, HolderWithdrawal>(
                            caller,
                            peer_directory(target.holder).await?,
                            target,
                            decode(request)?,
                            identity,
                        )
                    }
                    crate::fleet_creation_calls::ReadProjectCreationDecision::KIND => to_fleet!(crate::fleet_creation_calls::ReadProjectCreationDecision),
                    RetirementAdmission::KIND => to_fleet!(RetirementAdmission),
                    ClaimReservation::KIND => to_fleet!(ClaimReservation),
                    ClaimSettlement::KIND => to_fleet!(ClaimSettlement),
                    crate::fleet_lease_calls::AdvanceLeaseIncarnation::KIND => to_fleet!(crate::fleet_lease_calls::AdvanceLeaseIncarnation),
                    crate::fleet_lease_calls::ClaimGranted::KIND => to_fleet!(crate::fleet_lease_calls::ClaimGranted),
                    crate::fleet_lease_calls::PublishLeaseProjection::KIND => to_fleet!(crate::fleet_lease_calls::PublishLeaseProjection),
                    _ => Err(no_route()),
                }
            }
        }
    }
    async fn transmit(&self, call: SerializedObjectCall) -> Result<Vec<u8>, CallFailure> {
        #[cfg(feature = "conformance")]
        if let Ok(mut dispatches) = self.dispatches.lock() {
            dispatches.push(call.destination.clone());
        }
        self.io.object_call(call.destination, call.bytes).await
    }
}

impl<Io: PlatformIo> Clock for CoreHost<Io> {
    fn now(&self) -> SyncTime {
        self.io.now()
    }
}
impl<Io: PlatformIo> IdSource for CoreHost<Io> {
    fn new_id(&self) -> durable_steps::Uuid {
        self.io.new_id()
    }
}
impl<Io: PlatformIo> WakeScheduler for CoreHost<Io> {
    async fn wake_at(&self, at: SyncTime) -> Result<(), WakeFailure> {
        self.io.wake_at(at).await
    }
}
impl<Io: PlatformIo> Host for CoreHost<Io> {
    async fn dispatch<Call: CallTarget>(
        &self,
        target: Call,
        request: Call::Request,
        identity: CallId,
    ) -> Result<Call::Answer, CallFailure> {
        if Call::KIND == RegistrationAdmission::KIND {
            let _: RegistrationAdmission =
                decode(serde_json::to_value(&target).map_err(encode_refusal)?)?;
            let admission: fleet_sync_protocol::RegistrationAdmission =
                decode(serde_json::to_value(&request).map_err(encode_refusal)?)?;
            let answer = self.admit_registration(admission, identity).await?;
            return decode(serde_json::to_value(answer).map_err(encode_refusal)?);
        }
        if Call::KIND == ClaimSettlement::KIND {
            let _: ClaimSettlement =
                decode(serde_json::to_value(&target).map_err(encode_refusal)?)?;
            let settlement: fleet_sync_protocol::ClaimSettlement =
                decode(serde_json::to_value(&request).map_err(encode_refusal)?)?;
            if matches!(
                settlement.outcome,
                fleet_sync_protocol::SettleOutcome::Granted { .. }
            ) {
                return decode(self.settle_granted_claim(settlement, identity).await?);
            }
        }
        if Call::KIND == fleet_sync_receiver::FollowUpCall::<crate::CopyDirectorySummary>::KIND {
            let target = serde_json::to_value(target).map_err(encode_refusal)?;
            let request = serde_json::to_value(request).map_err(encode_refusal)?;
            return decode(self.copy_follow_up(target, request, identity).await?);
        }
        if Call::KIND
            == fleet_sync_delivery::PublishCall::<board_sync_protocol::BoardDeliveryTypes>::KIND
            && let Some(publication) = &self.publication
        {
            let target = decode(serde_json::to_value(target).map_err(encode_refusal)?)?;
            let request = decode(serde_json::to_value(request).map_err(encode_refusal)?)?;
            let answer = publication(target, request, identity).await?;
            return decode(serde_json::to_value(answer).map_err(encode_refusal)?);
        }
        #[cfg(feature = "conformance")]
        let recorded_identity = identity.clone();
        let call = self.classify(target, request, identity).await?;
        #[cfg(feature = "conformance")]
        if Call::KIND == crate::machine_enrollment::EndPeerSessions::KIND {
            self.revocation_probe
                .before_dispatch(
                    crate::machine_enrollment::conformance_revocation::RecordedLifecycleCall {
                        identity: recorded_identity.clone(),
                        destination: call.destination.clone(),
                        bytes: call.bytes.clone(),
                    },
                )
                .await
                .map_err(|_| CallFailure::AnswerLost)?;
        }
        #[cfg(feature = "conformance")]
        if Call::KIND == crate::mailbox_lifecycle::OpenPeerMailbox::KIND {
            self.revocation_probe
                .before_open_dispatch(
                    crate::machine_enrollment::conformance_revocation::RecordedLifecycleCall {
                        identity: recorded_identity.clone(),
                        destination: call.destination.clone(),
                        bytes: call.bytes.clone(),
                    },
                )
                .await
                .map_err(|_| CallFailure::AnswerLost)?;
        }
        #[cfg(feature = "conformance")]
        if Call::KIND == crate::mailbox_lifecycle::ClosePeerMailbox::KIND {
            self.revocation_probe
                .record_close_dispatch(
                    crate::machine_enrollment::conformance_revocation::RecordedLifecycleCall {
                        identity: recorded_identity,
                        destination: call.destination.clone(),
                        bytes: call.bytes.clone(),
                    },
                )
                .map_err(|_| CallFailure::AnswerLost)?;
        }
        let answer = self.transmit(call).await?;
        serde_json::from_slice(&answer).map_err(|_| CallFailure::AnswerLost)
    }
}

fn decode<Value: DeserializeOwned>(value: serde_json::Value) -> Result<Value, CallFailure> {
    serde_json::from_value(value).map_err(encode_refusal)
}

fn no_route() -> CallFailure {
    CallFailure::NotSent {
        reason: NotSentReason::Unreachable { last_seen: None },
    }
}

#[cfg(feature = "conformance")]
mod imported_unheld_fixture;
#[cfg(feature = "conformance")]
pub use imported_unheld_fixture::ConformanceBoardState;

#[cfg(feature = "conformance")]
mod imported_board_snapshot;
