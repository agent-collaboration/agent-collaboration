//! Read the real socket and alarm capabilities of a normally admitted session.

use super::{
    AdmissionOrigin, BindingAttachment, CloudflareStorageHost, ConnectionId,
    ConnectionRegistration, Host, SocketFailure,
};
use fleet_protocol::{AttachSequence, PeerId, SchemaSupport, SessionId};
use runtime_primitives::SyncTime;
use serde::Serialize;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SessionConnectionObservation {
    connection: ConnectionId,
    peer: PeerId,
    expires_at_millis: String,
    registration: ConnectionRegistration,
    admission_origin: AdmissionOrigin,
}

impl SessionConnectionObservation {
    pub(crate) fn belongs_to(&self, installation: &message_board::ServiceId) -> bool {
        &self.admission_origin.installation == installation
    }
    pub(crate) fn connection(&self) -> ConnectionId {
        self.connection
    }
    pub(crate) fn is_live(&self) -> bool {
        !matches!(self.registration, ConnectionRegistration::Closed)
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct AttachmentCodecObservation {
    connection: ConnectionId,
    peer: PeerId,
    session_id: SessionId,
    schema_support: String,
    attach_sequence: String,
    expires_at_millis: String,
    admission_origin: AdmissionOrigin,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SessionAlarmObservation {
    connection: ConnectionId,
    peer: PeerId,
    session_id: SessionId,
    alarm_at_millis: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    logical_now_millis: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    holder_reply_deadline_millis: Option<String>,
}

impl SessionAlarmObservation {
    pub(crate) fn with_holder_deadline(
        mut self,
        now: SyncTime,
        deadline: Option<SyncTime>,
    ) -> Self {
        self.logical_now_millis = Some(now.unix_millis().to_string());
        self.holder_reply_deadline_millis = deadline.map(|at| at.unix_millis().to_string());
        self
    }
}

impl<SharedHost: Host> CloudflareStorageHost<SharedHost> {
    #[cfg(feature = "conformance-faults")]
    pub(crate) async fn conformance_attachment_fault(
        &self,
        connection: ConnectionId,
        fault: crate::conformance_admission::AttachmentFault,
    ) -> Result<crate::shared_object_store::RegistrationSnapshot, SocketFailure> {
        use collaboration_central_core::ObjectHost as _;
        let socket = self.socket(connection)?;
        match fault {
            crate::conformance_admission::AttachmentFault::Missing => {
                socket.serialize_attachment(Option::<BindingAttachment>::None)
            }
            crate::conformance_admission::AttachmentFault::Invalid => {
                socket.serialize_attachment(serde_json::json!({"invalid":"attachment"}))
            }
        }
        .map_err(|_| SocketFailure::Closed)?;
        self.registrations().await
    }
    pub(crate) fn conformance_session_connections(
        &self,
    ) -> Result<Vec<SessionConnectionObservation>, SocketFailure> {
        Ok(self
            .state
            .get_websockets()
            .into_iter()
            .filter_map(|socket| {
                let attachment = Self::attachment(&socket).ok()?;
                let registration =
                    if Self::is_open(&socket) && !self.is_quarantined(attachment.connection) {
                        attachment.registration
                    } else {
                        ConnectionRegistration::Closed
                    };
                Some(SessionConnectionObservation {
                    connection: attachment.connection,
                    peer: attachment.peer,
                    expires_at_millis: attachment.expires_at.unix_millis().to_string(),
                    registration,
                    admission_origin: attachment.admission_origin,
                })
            })
            .collect())
    }

    pub(crate) fn conformance_attachment_codec(
        &self,
        connection: ConnectionId,
    ) -> Result<Vec<AttachmentCodecObservation>, SocketFailure> {
        let socket = self.socket(connection)?;
        let original = Self::attachment(&socket)?;
        let ConnectionRegistration::Attached { session_id, .. } = original.registration else {
            return Err(SocketFailure::NotConnected);
        };
        if !Self::is_open(&socket) || self.is_quarantined(connection) {
            return Err(SocketFailure::Closed);
        }
        // These synchronous workers-rs calls cannot interleave a frame event.
        // The original normal attachment is restored on either result path.
        let observed = (|| {
            let mut observations = Vec::new();
            for (support, sequence, expiry) in [
                (0, 1, 0),
                (9_007_199_254_740_993, 9_007_199_254_740_993, i64::MIN),
                (u64::MAX, u64::MAX, i64::MAX),
            ] {
                let expected = BindingAttachment {
                    connection,
                    peer: original.peer,
                    expires_at: SyncTime::from_unix_millis(expiry),
                    registration: ConnectionRegistration::Attaching {
                        capabilities: None,
                        session_id,
                        schema_support: SchemaSupport {
                            max_version: support,
                        },
                        attach_seq: AttachSequence::new(sequence)
                            .map_err(|_| SocketFailure::Closed)?,
                    },
                    admission_origin: original.admission_origin.clone(),
                };
                socket
                    .serialize_attachment(expected.clone())
                    .map_err(|_| SocketFailure::Closed)?;
                let actual = Self::attachment(&socket)?;
                if actual != expected
                    || self.binding(&socket)? != actual.binding()
                    || self.socket_origin(&socket)? != original.admission_origin
                {
                    return Err(SocketFailure::Closed);
                }
                let ConnectionRegistration::Attaching {
                    schema_support,
                    attach_seq,
                    ..
                } = actual.registration
                else {
                    return Err(SocketFailure::Closed);
                };
                observations.push(AttachmentCodecObservation {
                    connection: actual.connection,
                    peer: actual.peer,
                    session_id,
                    schema_support: schema_support.max_version.to_string(),
                    attach_sequence: attach_seq.value().to_string(),
                    expires_at_millis: actual.expires_at.unix_millis().to_string(),
                    admission_origin: actual.admission_origin,
                });
            }
            Ok(observations)
        })();
        socket
            .serialize_attachment(original)
            .map_err(|_| SocketFailure::Closed)?;
        observed
    }

    pub(crate) async fn conformance_session_alarm(
        &self,
        connection: ConnectionId,
    ) -> worker::Result<SessionAlarmObservation> {
        let convert = |error: SocketFailure| worker::Error::RustError(error.to_string());
        let socket = self.socket(connection).map_err(convert)?;
        let attachment = Self::attachment(&socket).map_err(convert)?;
        let ConnectionRegistration::Attached { session_id, .. } = attachment.registration else {
            return Err(convert(SocketFailure::NotConnected));
        };
        if !Self::is_open(&socket) || self.is_quarantined(connection) {
            return Err(convert(SocketFailure::Closed));
        }
        // The socket and this alarm are read from the same actual Durable Object
        // State; an echoed caller-supplied owner cannot satisfy this observation.
        let alarm = self.state.storage().get_alarm().await?;
        Ok(SessionAlarmObservation {
            connection: attachment.connection,
            peer: attachment.peer,
            session_id,
            alarm_at_millis: alarm.map(|at| at.to_string()),
            logical_now_millis: None,
            holder_reply_deadline_millis: None,
        })
    }
}
