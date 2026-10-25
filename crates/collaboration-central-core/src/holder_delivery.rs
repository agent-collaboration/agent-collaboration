//! The connection object's delivery face sends outside local exclusion.

use crate::{
    CallerKind, DeliveryKind, ObjectAddress, ObjectHost, ObjectRuntime, PlatformIo, SessionKind,
    caller_context::CallAuthority,
    object_call_envelope::{ObjectCallEnvelope, encode_refusal},
    object_runtime::ObjectWork,
};
use durable_steps::{
    AtomicStore, CallFailure, CallTarget, NotSentReason, ReborrowScope, StoreError,
    storage::TransactionScope,
};
use fleet_sync_lease::{HolderRelease, HolderWithdrawal};
#[cfg(feature = "conformance")]
use fleet_sync_protocol::FrameId;
use fleet_sync_protocol::{ClientAnswer, PeerId, ServerCall, ServerCallAnswer};
use sqlx_core::{
    arguments::IntoArguments, column::ColumnIndex, database::Database, decode::Decode,
    encode::Encode, types::Type,
};
use std::{marker::PhantomData, sync::Arc};

impl<Host: ObjectHost, Io: PlatformIo> ObjectRuntime<SessionKind, Host, Io>
where
    Host::Store: ReborrowScope,
    for<'transaction> <Host::Store as AtomicStore>::Scope<'transaction>:
        TransactionScope<Database = Host::Database>,
    <Host::Database as Database>::Arguments: IntoArguments<Host::Database>,
    for<'value> String:
        Type<Host::Database> + Encode<'value, Host::Database> + Decode<'value, Host::Database>,
    for<'value> Option<String>:
        Type<Host::Database> + Encode<'value, Host::Database> + Decode<'value, Host::Database>,
    for<'row> &'row str: ColumnIndex<<Host::Database as Database>::Row>,
{
    pub fn delivery(&self) -> Result<ObjectRuntime<DeliveryKind, Host, Io>, StoreError> {
        let ObjectWork::Session {
            host,
            shared_host,
            replies,
            installation,
            holder_answer_timeout,
        } = &self.work
        else {
            return Err(StoreError::Aborted {
                reason: "session runtime has a different face".into(),
            });
        };
        Ok(ObjectRuntime {
            work: ObjectWork::Session {
                host: host.clone(),
                shared_host: Arc::new(shared_host.delivery()),
                replies: replies.clone(),
                installation: installation.clone(),
                holder_answer_timeout: *holder_answer_timeout,
            },
            face: PhantomData,
        })
    }

    /// Test entry into the real delivery face; normal routing uses the Registry's
    /// committed peer directory instead of this supplied destination.
    #[cfg(feature = "conformance")]
    pub async fn conformance_holder_release(
        &self,
        holder: PeerId,
        request: fleet_sync_protocol::ReleaseRequest,
        identity: durable_steps::CallId,
    ) -> Result<fleet_sync_protocol::ReleaseAnswer, CallFailure> {
        let ObjectWork::Session { installation, .. } = &self.work else {
            return Err(unsent());
        };
        let call = crate::object_call_envelope::encode_shared_call::<
            crate::RegistryKind,
            DeliveryKind,
            HolderRelease,
        >(
            &crate::CallerContext::for_object(()),
            installation.clone(),
            HolderRelease {
                holder,
                database: request.database,
            },
            request,
            identity,
        )?;
        let bytes = self
            .delivery()
            .map_err(|_| unsent())?
            .on_call(call.bytes)
            .await?;
        serde_json::from_slice(&bytes).map_err(|_| CallFailure::AnswerLost)
    }

    /// Read-only causal observer of validated answer events and current waiters.
    #[cfg(feature = "conformance")]
    pub fn conformance_holder_observation(&self) -> Result<(usize, u64), StoreError> {
        let ObjectWork::Session { replies, .. } = &self.work else {
            return Err(StoreError::Aborted {
                reason: "session runtime has a different face".into(),
            });
        };
        replies.observation()
    }

    #[cfg(feature = "conformance")]
    pub fn conformance_holder_frames(&self) -> Result<Vec<FrameId>, StoreError> {
        let ObjectWork::Session { replies, .. } = &self.work else {
            return Err(StoreError::Aborted {
                reason: "session runtime has a different face".into(),
            });
        };
        replies.frames()
    }

    #[cfg(feature = "conformance")]
    pub fn conformance_holder_session_identity(&self) -> Result<(), StoreError> {
        let ObjectWork::Session { shared_host, .. } = &self.work else {
            return Err(StoreError::Aborted {
                reason: "session runtime has a different face".into(),
            });
        };
        crate::holder_reply_hub::session_identity_probe(shared_host.as_ref())
    }
}

enum HolderRequest {
    Release {
        holder: PeerId,
        request: fleet_sync_protocol::ReleaseRequest,
    },
    Withdrawal {
        holder: PeerId,
        request: fleet_sync_protocol::WithdrawRelease,
    },
}

enum HolderReplyKind {
    Release,
    Withdrawal,
}

impl<Host: ObjectHost, Io: PlatformIo> ObjectRuntime<DeliveryKind, Host, Io>
where
    Host::Store: ReborrowScope,
    for<'transaction> <Host::Store as AtomicStore>::Scope<'transaction>:
        TransactionScope<Database = Host::Database>,
    <Host::Database as Database>::Arguments: IntoArguments<Host::Database>,
    for<'value> String:
        Type<Host::Database> + Encode<'value, Host::Database> + Decode<'value, Host::Database>,
    for<'value> Option<String>:
        Type<Host::Database> + Encode<'value, Host::Database> + Decode<'value, Host::Database>,
    for<'row> &'row str: ColumnIndex<<Host::Database as Database>::Row>,
{
    pub async fn on_call(&self, bytes: Vec<u8>) -> Result<Vec<u8>, CallFailure> {
        let ObjectWork::Session {
            host,
            shared_host,
            replies,
            installation,
            holder_answer_timeout,
        } = &self.work
        else {
            return Err(unsent());
        };
        let invocation = ObjectCallEnvelope::decode(
            &bytes,
            ObjectAddress::Connection(installation.clone()),
            CallerKind::Delivery,
        )?;
        if matches!(
            invocation.kind.as_str(),
            crate::forward_calls::DeliverForward::KIND | crate::forward_calls::AnswerForward::KIND
        ) {
            return self.receive_forward_call(invocation).await;
        }
        if invocation.kind == crate::machine_notice_delivery::DeliverMachineNotice::KIND {
            return self.receive_machine_notice(invocation).await;
        }
        if invocation.kind
            == fleet_sync_delivery::PublishCall::<board_sync_protocol::BoardDeliveryTypes>::KIND
        {
            return self.mail_publication(invocation).await;
        }
        if !matches!(invocation.authority, CallAuthority::Object)
            || invocation.source != ObjectAddress::Registry
            || invocation.source_face != CallerKind::Registry
        {
            return Err(unsent());
        }
        if invocation.kind == crate::machine_enrollment::EndPeerSessions::KIND {
            return self.end_peer_sessions(invocation).await;
        }
        let request = match invocation.kind.as_str() {
            HolderRelease::KIND => {
                let target: HolderRelease =
                    serde_json::from_value(invocation.target).map_err(encode_refusal)?;
                let request: fleet_sync_protocol::ReleaseRequest =
                    serde_json::from_value(invocation.request).map_err(encode_refusal)?;
                if target.database != request.database {
                    return Err(unsent());
                }
                HolderRequest::Release {
                    holder: target.holder,
                    request,
                }
            }
            HolderWithdrawal::KIND => {
                let target: HolderWithdrawal =
                    serde_json::from_value(invocation.target).map_err(encode_refusal)?;
                let request = serde_json::from_value(invocation.request).map_err(encode_refusal)?;
                HolderRequest::Withdrawal {
                    holder: target.holder,
                    request,
                }
            }
            _ => return Err(unsent()),
        };
        let (holder, body, reply_kind) = match request {
            HolderRequest::Release { holder, request } => (
                holder,
                ServerCall::Release {
                    call: invocation.identity,
                    request,
                },
                HolderReplyKind::Release,
            ),
            HolderRequest::Withdrawal { holder, request } => (
                holder,
                ServerCall::WithdrawRelease {
                    call: invocation.identity,
                    request,
                },
                HolderReplyKind::Withdrawal,
            ),
        };
        let answer = crate::peer_request::begin_peer_request(
            host.clone(),
            shared_host.clone(),
            replies.clone(),
            holder,
            body,
            *holder_answer_timeout,
        )
        .await?
        .answer()
        .await?;
        match (reply_kind, answer) {
            (HolderReplyKind::Release, ClientAnswer::Crate(ServerCallAnswer::Release(answer))) => {
                serde_json::to_vec(&answer).map_err(|_| CallFailure::AnswerLost)
            }
            (
                HolderReplyKind::Withdrawal,
                ClientAnswer::Crate(ServerCallAnswer::WithdrawRelease(answer)),
            ) => serde_json::to_vec(&answer).map_err(|_| CallFailure::AnswerLost),
            _ => Err(CallFailure::AnswerLost),
        }
    }
}

fn unsent() -> CallFailure {
    CallFailure::NotSent {
        reason: NotSentReason::Unreachable { last_seen: None },
    }
}
