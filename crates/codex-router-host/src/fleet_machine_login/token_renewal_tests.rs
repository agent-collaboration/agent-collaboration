//! The real SessionPump and consumer envelopes carry each renewal and its bounded retry.
//! The transport answers as a central stand-in; it does not prove F10 admission.
use std::{
    collections::VecDeque,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    time::Duration,
};

use codex_router_core::redaction::SecretString;
use fleet_sync_protocol::{ClientFrame, CloseReason, SchemaSupport, ServerFrame};
use fleet_sync_session::{
    Bytes, ClientSessionState, ConnectFailure, ConnectionClosed, Endpoint, PeerCredential,
    SessionClient, SessionClientStore, SessionTransport, StoreError, TransportConnection,
};
use fleet_sync_tokio_runner::SessionPump;
use runtime_primitives::{Backoff, IdSource, SyncTime};
use tokio::sync::{Mutex, mpsc};

use super::super::{
    device_code_flow::{
        DeviceCodeAuthorization, DeviceLoginRequest, DevicePollResult, IssuerFailure,
    },
    login_clock::LoginFuture,
    login_publication::sync_directory,
    refresh_credential_store::RefreshCredentialStore,
};
use super::*;

struct FixedIds;
impl IdSource for FixedIds {
    fn new_id(&self) -> uuid::Uuid {
        uuid::Uuid::parse_str("019f0000-0000-7000-8000-000000000001").unwrap()
    }
}
struct MemoryState(Mutex<Option<ClientSessionState>>);
impl SessionClientStore for MemoryState {
    async fn load(&self) -> Result<Option<ClientSessionState>, StoreError> {
        Ok(self.0.lock().await.clone())
    }
    async fn save(&self, state: &ClientSessionState) -> Result<(), StoreError> {
        *self.0.lock().await = Some(state.clone());
        Ok(())
    }
}

#[derive(Clone)]
struct CentralStandIn {
    replies: Arc<Mutex<VecDeque<MachineTokenRenewal>>>,
    tokens: Arc<Mutex<Vec<String>>>,
}
struct CentralConnection {
    central: CentralStandIn,
    sender: mpsc::Sender<Bytes>,
    frames: mpsc::Receiver<Bytes>,
}
impl SessionTransport for CentralStandIn {
    type Connection = CentralConnection;
    async fn connect(
        &self,
        _: &Endpoint,
        _: &PeerCredential,
    ) -> Result<CentralConnection, ConnectFailure> {
        let (sender, frames) = mpsc::channel(8);
        Ok(CentralConnection {
            central: self.clone(),
            sender,
            frames,
        })
    }
}
impl TransportConnection for CentralConnection {
    async fn send(&mut self, bytes: Bytes) -> Result<(), ConnectionClosed> {
        let request: ClientFrame<BoardConsumerProtocol> =
            serde_json::from_slice(&bytes).map_err(|_| ConnectionClosed::Transport)?;
        let reply = match request {
            ClientFrame::Hello { session_id, .. } => ServerFrame::Welcome {
                session_id,
                databases: Vec::new(),
            },
            ClientFrame::Request {
                frame,
                body: OperationRequest::Consumer(BoardConsumerRequest::RenewMachineToken(token)),
            } => {
                self.central.tokens.lock().await.push(token.token);
                let reply = self
                    .central
                    .replies
                    .lock()
                    .await
                    .pop_front()
                    .ok_or(ConnectionClosed::Transport)?;
                ServerFrame::Answer {
                    frame,
                    body: OperationAnswer::Consumer(BoardConsumerAnswer::RenewMachineToken(reply)),
                }
            }
            _ => return Err(ConnectionClosed::Transport),
        };
        let bytes = serde_json::to_vec(&reply).map_err(|_| ConnectionClosed::Transport)?;
        self.sender
            .send(bytes.into())
            .await
            .map_err(|_| ConnectionClosed::Transport)
    }
    async fn receive(&mut self) -> Option<Result<Bytes, ConnectionClosed>> {
        self.frames.recv().await.map(Ok)
    }
    async fn close(self, _: CloseReason) {}
}

struct RotatingIssuer {
    refreshes: AtomicUsize,
    fail_on_second: bool,
}
impl MachineTokenIssuer for RotatingIssuer {
    fn authorize<'a>(
        &'a self,
        _: &'a DeviceLoginRequest,
    ) -> LoginFuture<'a, Result<DeviceCodeAuthorization, IssuerFailure>> {
        Box::pin(async { Err(IssuerFailure::InvalidGrant) })
    }
    fn poll<'a>(
        &'a self,
        _: &'a SecretString,
    ) -> LoginFuture<'a, Result<DevicePollResult, IssuerFailure>> {
        Box::pin(async { Err(IssuerFailure::InvalidGrant) })
    }
    fn refresh<'a>(
        &'a self,
        credential: &'a RefreshCredentialBundle,
    ) -> LoginFuture<'a, Result<IssuedMachineToken, IssuerFailure>> {
        Box::pin(async move {
            let generation = self.refreshes.fetch_add(1, Ordering::SeqCst) + 1;
            if self.fail_on_second && generation == 2 {
                return Err(IssuerFailure::InvalidGrant);
            }
            if credential.refresh_credential.expose_secret()
                != format!("refresh-{}", generation - 1)
            {
                return Err(IssuerFailure::InvalidGrant);
            }
            Ok(IssuedMachineToken {
                token: SecretString::new(format!("access-{generation}")),
                valid_until: SyncTime::from_unix_millis(600_000),
                credential: RefreshCredentialBundle {
                    machine_client_id: credential.machine_client_id.clone(),
                    refresh_credential: SecretString::new(format!("refresh-{generation}")),
                },
            })
        })
    }
}

async fn exercise(
    replies: Vec<MachineTokenRenewal>,
    fail_on_second: bool,
) -> (bool, usize, Vec<String>) {
    let root = tempfile::tempdir().unwrap();
    let central = CentralStandIn {
        replies: Arc::new(Mutex::new(replies.into())),
        tokens: Arc::new(Mutex::new(Vec::new())),
    };
    let client = SessionClient::<_, _, BoardConsumerProtocol>::load(
        central.clone(),
        MemoryState(Mutex::new(None)),
        &FixedIds,
        SchemaSupport { max_version: 1 },
        Backoff {
            initial: Duration::from_secs(1),
            max: Duration::from_secs(30),
            factor: 2,
        },
    )
    .await
    .unwrap();
    let (requests, mut events, pump) = SessionPump::new(
        client,
        Endpoint::new("http://central.example.invalid/session").unwrap(),
        PeerCredential::new("fixture-only").unwrap(),
        Duration::from_secs(30),
    );
    let (stop, stopped) = watch::channel(false);
    let task = tokio::spawn(pump.run(stopped));
    assert!(matches!(
        events.recv().await,
        Some(fleet_sync_session::ClientEvent::Welcome { .. })
    ));
    let (_welcomed, readiness) = watch::channel(true);
    let issuer = Arc::new(RotatingIssuer {
        refreshes: AtomicUsize::new(0),
        fail_on_second,
    });
    let credential = RefreshCredentialBundle {
        machine_client_id: "019f0000-0000-7000-8000-000000000010"
            .to_owned()
            .try_into()
            .unwrap(),
        refresh_credential: SecretString::new("refresh-0"),
    };
    let publication = Arc::new(MachineLoginPublication {
        root: root.path().to_owned(),
        credentials: Arc::new(RefreshCredentialStore::MemoryOnly),
        sync_directory,
    });

    let result = renew_machine_token(
        issuer.clone(),
        publication,
        credential,
        Some(MachineRenewalConnection {
            requests,
            welcomed: readiness,
        }),
    )
    .await;

    let tokens = central.tokens.lock().await.clone();
    stop.send_replace(true);
    task.await.unwrap().unwrap();
    (
        result.is_ok(),
        issuer.refreshes.load(Ordering::SeqCst),
        tokens,
    )
}

#[tokio::test]
async fn fleet_machine_login_renewal_uses_rotated_client_and_retries_each_nonterminal_refusal_once()
{
    for reason in [
        MachineTokenRenewalRefusal::TokenInvalid {},
        MachineTokenRenewalRefusal::TokenExpired {},
        MachineTokenRenewalRefusal::WrongClient {},
        MachineTokenRenewalRefusal::WrongInstallation {},
    ] {
        let (success, refreshes, tokens) = exercise(
            vec![
                MachineTokenRenewal::Refused { reason },
                MachineTokenRenewal::Renewed {},
            ],
            false,
        )
        .await;
        assert!(success);
        assert_eq!(refreshes, 2);
        assert_eq!(tokens, vec!["access-1", "access-2"]);
        let (success, refreshes, tokens) = exercise(
            vec![
                MachineTokenRenewal::Refused { reason },
                MachineTokenRenewal::Refused { reason },
            ],
            false,
        )
        .await;
        assert!(!success);
        assert_eq!(refreshes, 2);
        assert_eq!(tokens, vec!["access-1", "access-2"]);
    }
}

#[tokio::test]
async fn fleet_machine_login_renewal_stops_on_inactive_enrollment_or_failed_extra_refresh() {
    let (success, refreshes, tokens) = exercise(
        vec![MachineTokenRenewal::Refused {
            reason: MachineTokenRenewalRefusal::EnrollmentNotActive {},
        }],
        false,
    )
    .await;
    assert!(!success);
    assert_eq!(refreshes, 1);
    assert_eq!(tokens, vec!["access-1"]);
    let (success, refreshes, tokens) = exercise(
        vec![MachineTokenRenewal::Refused {
            reason: MachineTokenRenewalRefusal::TokenInvalid {},
        }],
        true,
    )
    .await;
    assert!(!success);
    assert_eq!(refreshes, 2);
    assert_eq!(tokens, vec!["access-1"]);
}
