//! One Host lifetime owns a pending code, its observers and the active refresh family.
use std::{future::pending, sync::Arc};

use fleet_identity_protocol::{InstallationId, OrganizationId};
use fleet_sync_protocol::PeerId;
use message_board::MachineLabel;
use runtime_primitives::SyncTime;
use tokio::{
    sync::{mpsc, watch},
    task::JoinHandle,
};
use tokio_util::sync::CancellationToken;

use super::{
    device_code_flow::{
        DeviceCodeFailure, DeviceLoginRequest, IssuedMachineToken, MachineTokenIssuer,
        RefreshCredentialBundle, renewal_deadline, run_device_code_login,
    },
    login_clock::{FleetLoginClock, LoginFuture},
    login_contract::{FleetLoginEvent, FleetLoginRefusal, RefreshCredentialKeeping},
    login_files::MachineLoginMetadata,
    login_publication::MachineLoginPublication,
    machine_token_transport::{MachineAccessToken, MachineTokenSource},
    refresh_credential_store::RefreshCredentialError,
    token_renewal::{MachineRenewalConnection, renew_machine_token},
};

pub(super) struct MachineLoginIdentity {
    pub installation: InstallationId,
    pub peer: PeerId,
}

enum LoginCommand {
    Observe {
        organization_id: OrganizationId,
        label: MachineLabel,
        events: mpsc::Sender<FleetLoginEvent>,
    },
    AttachConnection(Option<MachineRenewalConnection>),
}

#[derive(Clone)]
pub(crate) struct MachineLoginHandle {
    commands: mpsc::UnboundedSender<LoginCommand>,
    pub source: MachineTokenSource,
}

impl MachineLoginHandle {
    pub(crate) fn observe(
        &self,
        organization_id: OrganizationId,
        label: MachineLabel,
    ) -> mpsc::Receiver<FleetLoginEvent> {
        let (events, receiver) = mpsc::channel(4);
        let _submitted = self.commands.send(LoginCommand::Observe {
            organization_id,
            label,
            events,
        });
        receiver
    }

    pub(crate) fn attach_connection(&self, connection: Option<MachineRenewalConnection>) {
        let _submitted = self
            .commands
            .send(LoginCommand::AttachConnection(connection));
    }
}

pub(crate) struct RunningMachineLogin {
    pub handle: MachineLoginHandle,
    stop: CancellationToken,
    task: Option<JoinHandle<()>>,
}

impl RunningMachineLogin {
    pub(super) fn start(
        identity: MachineLoginIdentity,
        issuer: Arc<dyn MachineTokenIssuer>,
        clock: Arc<dyn FleetLoginClock>,
        publication: Arc<MachineLoginPublication>,
        saved: Option<MachineLoginMetadata>,
    ) -> Self {
        let (commands, receiver) = mpsc::unbounded_channel();
        let (token, source) = MachineTokenSource::channel(clock.clone());
        let stop = CancellationToken::new();
        let owner = MachineLoginOwner::new(identity, issuer, clock, publication, saved, token);
        let task_stop = stop.clone();
        let task = tokio::spawn(owner.run(receiver, task_stop));
        Self {
            handle: MachineLoginHandle { commands, source },
            stop,
            task: Some(task),
        }
    }

    pub(crate) async fn shutdown(mut self) -> std::io::Result<()> {
        self.stop.cancel();
        if let Some(task) = self.task.take() {
            task.await.map_err(std::io::Error::other)?;
        }
        Ok(())
    }
}
impl Drop for RunningMachineLogin {
    fn drop(&mut self) {
        self.stop.cancel();
    }
}

struct PendingLogin {
    organization_id: OrganizationId,
    code: Option<FleetLoginEvent>,
    observers: Vec<mpsc::Sender<FleetLoginEvent>>,
    codes: mpsc::UnboundedReceiver<FleetLoginEvent>,
    future: LoginFuture<'static, Result<IssuedMachineToken, DeviceCodeFailure>>,
}

enum SavedLogin {
    Fresh,
    RequiresApproval(MachineLoginMetadata),
    Refreshable {
        metadata: MachineLoginMetadata,
        credential: RefreshCredentialBundle,
    },
}
impl SavedLogin {
    fn metadata(&self) -> Option<&MachineLoginMetadata> {
        match self {
            Self::Fresh => None,
            Self::RequiresApproval(metadata) | Self::Refreshable { metadata, .. } => Some(metadata),
        }
    }
    fn require_approval(&mut self) {
        if let Some(metadata) = self.metadata().cloned() {
            *self = Self::RequiresApproval(metadata);
        }
    }
}

struct MachineLoginOwner {
    identity: MachineLoginIdentity,
    issuer: Arc<dyn MachineTokenIssuer>,
    clock: Arc<dyn FleetLoginClock>,
    publication: Arc<MachineLoginPublication>,
    keeping: Result<RefreshCredentialKeeping, FleetLoginRefusal>,
    saved: SavedLogin,
    token: watch::Sender<MachineAccessToken>,
    connection: Option<MachineRenewalConnection>,
    pending: Option<PendingLogin>,
    renewal: Option<LoginFuture<'static, Result<IssuedMachineToken, ()>>>,
    renew_at: Option<SyncTime>,
}

impl MachineLoginOwner {
    fn new(
        identity: MachineLoginIdentity,
        issuer: Arc<dyn MachineTokenIssuer>,
        clock: Arc<dyn FleetLoginClock>,
        publication: Arc<MachineLoginPublication>,
        saved: Option<MachineLoginMetadata>,
        token: watch::Sender<MachineAccessToken>,
    ) -> Self {
        let mut keeping = publication
            .credentials
            .keeping()
            .map_err(credential_refusal);
        let saved = match saved {
            None => SavedLogin::Fresh,
            Some(metadata) => match publication.credentials.read(&metadata.enrollment_id) {
                Ok(Some(credential)) => SavedLogin::Refreshable {
                    metadata,
                    credential,
                },
                Ok(None) => SavedLogin::RequiresApproval(metadata),
                Err(error) => {
                    keeping = Err(credential_refusal(error));
                    SavedLogin::RequiresApproval(metadata)
                }
            },
        };
        let renew_at = matches!(saved, SavedLogin::Refreshable { .. }).then(|| clock.now());
        Self {
            identity,
            issuer,
            clock,
            publication,
            keeping,
            saved,
            token,
            connection: None,
            pending: None,
            renewal: None,
            renew_at,
        }
    }

    async fn run(
        mut self,
        mut commands: mpsc::UnboundedReceiver<LoginCommand>,
        stop: CancellationToken,
    ) {
        loop {
            let clock = self.clock.clone();
            let deadline = self.renew_at;
            tokio::select! {
                biased;
                _ = stop.cancelled() => break,
                command = commands.recv() => match command {
                    Some(LoginCommand::Observe { organization_id, label, events }) => self.observe(organization_id, label, events),
                    Some(LoginCommand::AttachConnection(connection)) => self.connection = connection,
                    None => break,
                },
                progress = next_login_progress(&mut self.pending), if self.pending.is_some() => {
                    match progress {
                        LoginProgress::Code(code) => self.show_code(code),
                        LoginProgress::Complete(result) => self.complete_login(result),
                    }
                },
                result = complete_renewal(&mut self.renewal), if self.renewal.is_some() => {
                    self.renewal = None;
                    match result { Ok(issued) => self.use_token(issued), Err(()) => self.require_login() }
                },
                _ = wait_for_renewal(clock.as_ref(), deadline), if deadline.is_some() => self.start_renewal(),
            }
        }
        // Dropping the pending future abandons it. No device code or pending
        // state is saved, and no replacement Host can replay that request.
        self.token.send_replace(MachineAccessToken::NotLoggedIn);
    }

    fn observe(
        &mut self,
        organization_id: OrganizationId,
        label: MachineLabel,
        events: mpsc::Sender<FleetLoginEvent>,
    ) {
        let bound = self
            .saved
            .metadata()
            .map(|metadata| &metadata.organization_id);
        if let Some(bound) = bound
            && *bound != organization_id
        {
            self.refuse_observer(
                events,
                FleetLoginRefusal::AlreadyInOrganization {
                    organization_id: bound.clone(),
                },
                &organization_id,
            );
            return;
        }
        let keeping = match &self.keeping {
            Ok(keeping) => *keeping,
            Err(reason) => {
                self.refuse_observer(events, reason.clone(), &organization_id);
                return;
            }
        };
        if let Some(login) = &mut self.pending {
            if let Some(code) = &login.code {
                let _sent = events.try_send(code.clone());
            }
            login.observers.retain(|observer| !observer.is_closed());
            login.observers.push(events);
            return;
        }
        // These futures are polled only by this owner. Dropping an older one
        // before a login starts prevents any late result from publishing it.
        self.renewal = None;
        self.renew_at = None;
        self.token.send_replace(MachineAccessToken::NotLoggedIn);
        self.saved.require_approval();
        let request = DeviceLoginRequest {
            organization_id: organization_id.clone(),
            installation: self.identity.installation.clone(),
            peer: self.identity.peer,
            label,
        };
        tracing::info!(event = "machineLogin", outcome = "started", organization_id = request.organization_id.as_str(), installation = request.installation.as_str(), peer = %request.peer, "machine login started");
        let issuer = self.issuer.clone();
        let clock = self.clock.clone();
        let (codes, receiver) = mpsc::unbounded_channel();
        let future = Box::pin(async move {
            run_device_code_login(issuer.as_ref(), clock.as_ref(), &request, keeping, |code| {
                let _sent = codes.send(code);
            })
            .await
        });
        self.pending = Some(PendingLogin {
            organization_id,
            code: None,
            observers: vec![events],
            codes: receiver,
            future,
        });
    }

    fn show_code(&mut self, code: FleetLoginEvent) {
        if let Some(login) = &mut self.pending {
            login
                .observers
                .retain(|observer| observer.try_send(code.clone()).is_ok());
            login.code = Some(code);
        }
    }

    fn complete_login(&mut self, result: Result<IssuedMachineToken, DeviceCodeFailure>) {
        // Code and terminal may become ready in the same poll. Deliver the
        // display event first even when the issuer answers immediately.
        if let Some(login) = &mut self.pending
            && let Ok(code) = login.codes.try_recv()
        {
            self.show_code(code);
        }
        let Some(login) = self.pending.take() else {
            return;
        };
        let requested_organization = login.organization_id.clone();
        let event = match result {
            Err(DeviceCodeFailure::Refused(reason)) => FleetLoginEvent::Refused { reason },
            Err(DeviceCodeFailure::OutcomeUnknown(context)) => {
                FleetLoginEvent::OutcomeUnknown { context }
            }
            Ok(issued) => {
                let metadata = MachineLoginMetadata {
                    organization_id: login.organization_id,
                    enrollment_id: issued.credential.machine_client_id.clone(),
                };
                let keeping = match self.keeping {
                    Ok(keeping) => keeping,
                    Err(_) => return,
                };
                self.saved = SavedLogin::RequiresApproval(metadata.clone());
                match self
                    .publication
                    .publish_login(&metadata, &issued.credential, keeping)
                {
                    Err(context) => FleetLoginEvent::OutcomeUnknown { context },
                    Ok(()) => {
                        self.use_token(issued);
                        FleetLoginEvent::LoggedIn {
                            organization_id: metadata.organization_id,
                            enrollment_id: metadata.enrollment_id,
                            keeping,
                        }
                    }
                }
            }
        };
        if !matches!(event, FleetLoginEvent::LoggedIn { .. }) {
            self.require_login();
        }
        for observer in login.observers {
            let _sent = observer.try_send(event.clone());
        }
        self.record_login_ending(&event, &requested_organization);
    }

    fn refuse_observer(
        &self,
        events: mpsc::Sender<FleetLoginEvent>,
        reason: FleetLoginRefusal,
        organization_id: &OrganizationId,
    ) {
        let event = FleetLoginEvent::Refused { reason };
        self.record_login_ending(&event, organization_id);
        let _sent = events.try_send(event);
    }

    fn record_login_ending(
        &self,
        event: &FleetLoginEvent,
        requested_organization: &OrganizationId,
    ) {
        match event {
            FleetLoginEvent::LoggedIn {
                organization_id,
                enrollment_id,
                keeping,
            } => {
                tracing::info!(event = "machineLogin", outcome = "loggedIn", organization_id = organization_id.as_str(), enrollment_id = enrollment_id.as_str(), installation = self.identity.installation.as_str(), keeping = ?keeping, "machine login ended")
            }
            FleetLoginEvent::Refused { reason } => {
                tracing::info!(event = "machineLogin", outcome = "refused", reason = ?reason, organization_id = requested_organization.as_str(), installation = self.identity.installation.as_str(), "machine login ended")
            }
            FleetLoginEvent::OutcomeUnknown { context } => {
                tracing::warn!(event = "machineLogin", outcome = "outcomeUnknown", context = ?context, organization_id = requested_organization.as_str(), installation = self.identity.installation.as_str(), "machine login ended")
            }
            FleetLoginEvent::Code { .. } => {}
        }
    }

    fn start_renewal(&mut self) {
        self.renew_at = None;
        if let SavedLogin::Refreshable { credential, .. } = &self.saved {
            tracing::info!(
                event = "machineTokenRenewal",
                outcome = "started",
                enrollment_id = credential.machine_client_id.as_str(),
                installation = self.identity.installation.as_str(),
                "machine token refresh started"
            );
            self.renewal = Some(Box::pin(renew_machine_token(
                self.issuer.clone(),
                self.publication.clone(),
                credential.clone(),
                self.connection.clone(),
            )));
        }
    }

    fn use_token(&mut self, issued: IssuedMachineToken) {
        let Some(metadata) = self.saved.metadata().cloned() else {
            return;
        };
        self.renew_at = Some(renewal_deadline(self.clock.now(), issued.valid_until));
        let enrollment_id = metadata.enrollment_id.clone();
        self.saved = SavedLogin::Refreshable {
            metadata,
            credential: issued.credential,
        };
        self.token.send_replace(MachineAccessToken::Granted {
            enrollment_id,
            token: issued.token,
            expires_at: issued.valid_until,
        });
    }

    fn require_login(&mut self) {
        self.renew_at = None;
        self.saved.require_approval();
        self.token.send_replace(MachineAccessToken::NotLoggedIn);
    }
}

fn credential_refusal(error: RefreshCredentialError) -> FleetLoginRefusal {
    use codex_router_secret_store::classified_store_open::UnusableCause;
    FleetLoginRefusal::StoreUnusable {
        cause: match error {
            RefreshCredentialError::Unusable { cause } => cause,
            RefreshCredentialError::DamagedCredential | RefreshCredentialError::WriteFailed => {
                UnusableCause::InvalidCredentialData
            }
        },
    }
}

enum LoginProgress {
    Code(FleetLoginEvent),
    Complete(Result<IssuedMachineToken, DeviceCodeFailure>),
}
async fn next_login_progress(login: &mut Option<PendingLogin>) -> LoginProgress {
    let Some(login) = login else {
        return pending().await;
    };
    tokio::select! {
        biased;
        Some(code) = login.codes.recv(), if login.code.is_none() => LoginProgress::Code(code),
        result = login.future.as_mut() => LoginProgress::Complete(result),
    }
}
async fn complete_renewal(
    renewal: &mut Option<LoginFuture<'static, Result<IssuedMachineToken, ()>>>,
) -> Result<IssuedMachineToken, ()> {
    match renewal {
        Some(renewal) => renewal.as_mut().await,
        None => pending().await,
    }
}
async fn wait_for_renewal(clock: &dyn FleetLoginClock, deadline: Option<SyncTime>) {
    match deadline {
        Some(deadline) => {
            let _waited = clock.wait_until(deadline).await;
        }
        None => pending().await,
    }
}

#[cfg(test)]
#[path = "login_owner_tests.rs"]
mod tests;
