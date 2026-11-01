//! One Host lifetime owns pending login, adoption and the active refresh family.
use std::{future::pending, sync::Arc};

use codex_router_fleet_state::fleet_login::{SavedFleetLogin, SignInAddress};
use fleet_identity_protocol::{InstallationId, MachineEnvironment, OrganizationId};
use fleet_sync_protocol::PeerId;
use message_board::MachineLabel;
use runtime_primitives::SyncTime;
use tokio::{
    sync::{mpsc, watch},
    task::JoinHandle,
};
use tokio_util::sync::CancellationToken;
use tracing::instrument::WithSubscriber;

use super::{
    device_code_flow::{DeviceCodeFailure, DeviceLoginRequest, renewal_deadline},
    login_clock::{FleetLoginClock, LoginFuture},
    login_contract::{FleetLoginEvent, FleetLoginRefusal, RefreshCredentialKeeping},
    login_owner_state::{LoginObservers, SavedLogin},
    login_work::{ConfirmedLogin, IssuerSource, MachineLoginWork, RefreshedLogin},
    machine_token_transport::{MachineAccessToken, MachineTokenSource},
    refresh_credential_store::RefreshCredentialError,
    token_renewal::MachineRenewalConnection,
};

pub(super) struct MachineLoginIdentity {
    pub installation: InstallationId,
    pub peer: PeerId,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum MachineLoginRoute {
    AwaitingLogin,
    Learned(Arc<SavedFleetLogin>),
}
impl MachineLoginRoute {
    pub(crate) fn saved(&self) -> Option<&Arc<SavedFleetLogin>> {
        match self {
            Self::Learned(saved) => Some(saved),
            Self::AwaitingLogin => None,
        }
    }
}
enum LoginCommand {
    Observe {
        sign_in: Option<SignInAddress>,
        organization_id: OrganizationId,
        label: MachineLabel,
        machine_environment: Option<MachineEnvironment>,
        events: mpsc::Sender<FleetLoginEvent>,
    },
    AttachConnection(Option<MachineRenewalConnection>),
    RouteAdopted(Arc<SavedFleetLogin>),
    RouteUnavailable(Arc<SavedFleetLogin>),
    SuspendRoute(tokio::sync::oneshot::Sender<()>),
}

#[derive(Clone)]
pub(crate) struct MachineLoginHandle {
    commands: mpsc::UnboundedSender<LoginCommand>,
    pub source: MachineTokenSource,
    route: watch::Receiver<MachineLoginRoute>,
}
impl MachineLoginHandle {
    pub(crate) fn observe(
        &self,
        sign_in: Option<SignInAddress>,
        organization_id: OrganizationId,
        label: MachineLabel,
        machine_environment: Option<MachineEnvironment>,
    ) -> mpsc::Receiver<FleetLoginEvent> {
        let (events, receiver) = mpsc::channel(4);
        let _submitted = self.commands.send(LoginCommand::Observe {
            sign_in,
            organization_id,
            label,
            machine_environment,
            events,
        });
        receiver
    }
    pub(crate) fn route(&self) -> MachineLoginRoute {
        self.route.borrow().clone()
    }
    pub(crate) fn route_changes(&self) -> watch::Receiver<MachineLoginRoute> {
        self.route.clone()
    }
    pub(crate) fn attach_connection(&self, connection: Option<MachineRenewalConnection>) {
        let _submitted = self
            .commands
            .send(LoginCommand::AttachConnection(connection));
    }
    pub(crate) fn route_adopted(&self, saved: Arc<SavedFleetLogin>) {
        let _submitted = self.commands.send(LoginCommand::RouteAdopted(saved));
    }
    pub(crate) fn route_unavailable(&self, saved: Arc<SavedFleetLogin>) {
        let _submitted = self.commands.send(LoginCommand::RouteUnavailable(saved));
    }
    pub(crate) async fn suspend_route(&self) {
        let (done, settled) = tokio::sync::oneshot::channel();
        if self.commands.send(LoginCommand::SuspendRoute(done)).is_ok() {
            let _settled = settled.await;
        }
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
        work: MachineLoginWork,
        saved: Option<SavedFleetLogin>,
    ) -> Self {
        let (commands, receiver) = mpsc::unbounded_channel();
        let (token, source) = MachineTokenSource::channel(work.clock.clone());
        let initial = saved
            .as_ref()
            .map_or(MachineLoginRoute::AwaitingLogin, |saved| {
                MachineLoginRoute::Learned(Arc::new(saved.clone()))
            });
        let (route, changes) = watch::channel(initial);
        let stop = CancellationToken::new();
        let owner = MachineLoginOwner::new(identity, work, saved, token, route);
        let task_stop = stop.clone();
        let task = tokio::spawn(owner.run(receiver, task_stop).with_current_subscriber());
        Self {
            handle: MachineLoginHandle {
                commands,
                source,
                route: changes,
            },
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
    listeners: LoginObservers,
    codes: mpsc::UnboundedReceiver<FleetLoginEvent>,
    future: LoginFuture<'static, Result<ConfirmedLogin, DeviceCodeFailure>>,
}
struct AwaitingRoute {
    saved: Arc<SavedFleetLogin>,
    listeners: LoginObservers,
}

struct MachineLoginOwner {
    identity: MachineLoginIdentity,
    work: MachineLoginWork,
    keeping: Result<RefreshCredentialKeeping, FleetLoginRefusal>,
    saved: SavedLogin,
    token: watch::Sender<MachineAccessToken>,
    route: watch::Sender<MachineLoginRoute>,
    adopted: Option<Arc<SavedFleetLogin>>,
    connection: Option<MachineRenewalConnection>,
    pending: Option<PendingLogin>,
    awaiting: Option<AwaitingRoute>,
    renewal: Option<LoginFuture<'static, Result<RefreshedLogin, ()>>>,
    renew_at: Option<SyncTime>,
}
impl MachineLoginOwner {
    fn new(
        identity: MachineLoginIdentity,
        work: MachineLoginWork,
        saved: Option<SavedFleetLogin>,
        token: watch::Sender<MachineAccessToken>,
        route: watch::Sender<MachineLoginRoute>,
    ) -> Self {
        let mut keeping = work
            .publication
            .credentials
            .keeping()
            .map_err(credential_refusal);
        let saved = match saved.map(Arc::new) {
            None => SavedLogin::Fresh,
            Some(saved) => match work.publication.credentials.read(&saved.enrollment_id) {
                Ok(Some(credential)) => SavedLogin::Restoring { saved, credential },
                Ok(None) => SavedLogin::RequiresApproval(saved),
                Err(error) => {
                    keeping = Err(credential_refusal(error));
                    SavedLogin::RequiresApproval(saved)
                }
            },
        };
        let renew_at = matches!(saved, SavedLogin::Restoring { .. }).then(|| work.clock.now());
        Self {
            identity,
            work,
            keeping,
            saved,
            token,
            route,
            adopted: None,
            connection: None,
            pending: None,
            awaiting: None,
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
            let clock = self.work.clock.clone();
            let deadline = self.renew_at;
            tokio::select! {
                biased;
                _ = stop.cancelled() => break,
                command = commands.recv() => match command {
                    Some(LoginCommand::Observe { sign_in, organization_id, label, machine_environment, events }) => self.observe(sign_in, organization_id, label, machine_environment, events).await,
                    Some(LoginCommand::AttachConnection(connection)) => self.connection = connection,
                    Some(LoginCommand::RouteAdopted(saved)) => self.adopt(saved),
                    Some(LoginCommand::SuspendRoute(done)) => { self.suspend_route(); let _settled = done.send(()); },
                    Some(LoginCommand::RouteUnavailable(saved)) => if self.saved.saved() == Some(&saved) { self.require_login(); },
                    None => break,
                },
                progress = next_login_progress(&mut self.pending), if self.pending.is_some() => match progress {
                    LoginProgress::Code(code) => if let Some(login) = &mut self.pending { login.listeners.show(code); },
                    LoginProgress::Complete(result) => self.complete_login(result),
                },
                result = complete_renewal(&mut self.renewal), if self.renewal.is_some() => {
                    self.renewal = None;
                    match result { Ok(refreshed) => self.use_token(refreshed), Err(()) => self.require_login() }
                },
                _ = wait_for_renewal(clock.as_ref(), deadline), if deadline.is_some() => self.start_renewal(),
            }
        }
        self.token.send_replace(MachineAccessToken::NotLoggedIn);
    }
    async fn observe(
        &mut self,
        sign_in: Option<SignInAddress>,
        organization_id: OrganizationId,
        label: MachineLabel,
        machine_environment: Option<MachineEnvironment>,
        events: mpsc::Sender<FleetLoginEvent>,
    ) {
        if let Some(saved) = self.saved.saved()
            && saved.organization_id != organization_id
        {
            self.refuse(
                events,
                FleetLoginRefusal::AlreadyInOrganization {
                    organization_id: saved.organization_id.clone(),
                },
            );
            return;
        }
        if let Some(login) = &mut self.pending {
            login.listeners.attach(events);
            return;
        }
        if let Some(login) = &mut self.awaiting {
            login.listeners.attach(events);
            return;
        }
        // A new explicit command reads what genuinely committed, including a
        // prior uncertain save. It starts a fresh code, never replays the old one.
        match self.work.publication.repository.load_fleet_login().await {
            Ok(Some(saved)) => {
                if saved.organization_id != organization_id {
                    self.refuse(
                        events,
                        FleetLoginRefusal::AlreadyInOrganization {
                            organization_id: saved.organization_id,
                        },
                    );
                    return;
                }
                if self.saved.saved().is_none_or(|current| **current != saved) {
                    self.saved = SavedLogin::RequiresApproval(Arc::new(saved));
                }
            }
            Ok(None) => {}
            Err(_) => {
                self.require_login();
                self.refuse(events, FleetLoginRefusal::ConnectionUnavailable {});
                return;
            }
        }
        let Some(sign_in) =
            sign_in.or_else(|| self.saved.saved().map(|saved| saved.sign_in.clone()))
        else {
            self.refuse(events, FleetLoginRefusal::NotConfigured {});
            return;
        };
        self.renewal = None;
        self.renew_at = None;
        self.adopted = None;
        self.token.send_replace(MachineAccessToken::NotLoggedIn);
        if self.saved.saved().is_none() {
            self.route.send_replace(MachineLoginRoute::AwaitingLogin);
        }
        self.saved.require_approval();
        let keeping = match &self.keeping {
            Ok(keeping) => *keeping,
            Err(reason) => {
                self.refuse(events, reason.clone());
                return;
            }
        };
        let request = DeviceLoginRequest {
            organization_id,
            installation: self.identity.installation.clone(),
            peer: self.identity.peer,
            label,
        };
        tracing::info!(
            event = "machineLogin",
            outcome = "started",
            organization_id = request.organization_id.as_str(),
            installation = request.installation.as_str(),
            "machine login started"
        );
        let (codes, receiver) = mpsc::unbounded_channel();
        self.pending = Some(PendingLogin {
            listeners: LoginObservers {
                code: None,
                observers: vec![events],
            },
            codes: receiver,
            future: Box::pin(self.work.clone().login(
                request,
                sign_in,
                machine_environment,
                keeping,
                codes,
            )),
        });
    }
    fn complete_login(&mut self, result: Result<ConfirmedLogin, DeviceCodeFailure>) {
        let Some(mut pending) = self.pending.take() else {
            return;
        };
        if let Ok(code) = pending.codes.try_recv() {
            pending.listeners.show(code);
        }
        match result {
            Err(failure) => {
                self.require_login();
                let event = match failure {
                    DeviceCodeFailure::Refused(reason) => FleetLoginEvent::Refused { reason },
                    DeviceCodeFailure::OutcomeUnknown(context) => {
                        FleetLoginEvent::OutcomeUnknown { context }
                    }
                };
                self.record_ending(&event);
                pending.listeners.finish(event);
            }
            Ok(confirmed) => {
                let saved = Arc::new(confirmed.saved);
                self.saved = SavedLogin::Ready {
                    saved: saved.clone(),
                    credential: confirmed.issued.credential.clone(),
                    issuer: confirmed.issuer.clone(),
                };
                self.awaiting = Some(AwaitingRoute {
                    saved: saved.clone(),
                    listeners: pending.listeners,
                });
                self.use_token(RefreshedLogin {
                    issued: confirmed.issued,
                    issuer: confirmed.issuer,
                });
                self.route.send_replace(MachineLoginRoute::Learned(saved));
            }
        }
    }
    fn suspend_route(&mut self) {
        self.adopted = None;
        self.token.send_if_modified(|token| {
            let previous = std::mem::replace(token, MachineAccessToken::NotLoggedIn);
            match previous {
                MachineAccessToken::Granted {
                    enrollment_id,
                    token: value,
                    expires_at,
                } => {
                    *token = MachineAccessToken::AwaitingRoute {
                        enrollment_id,
                        token: value,
                        expires_at,
                    };
                    true
                }
                other => {
                    *token = other;
                    false
                }
            }
        });
    }
    fn adopt(&mut self, saved: Arc<SavedFleetLogin>) {
        if self.saved.saved() != Some(&saved) {
            return;
        }
        self.adopted = Some(saved.clone());
        self.token.send_if_modified(|token| {
            let previous = std::mem::replace(token, MachineAccessToken::NotLoggedIn);
            match previous {
                MachineAccessToken::AwaitingRoute {
                    enrollment_id,
                    token: value,
                    expires_at,
                } if enrollment_id == saved.enrollment_id => {
                    *token = MachineAccessToken::Granted {
                        enrollment_id,
                        token: value,
                        expires_at,
                    };
                    true
                }
                other => {
                    *token = other;
                    false
                }
            }
        });
        self.finish_adopted_login();
    }
    fn finish_adopted_login(&mut self) {
        if !matches!(&*self.token.borrow(), MachineAccessToken::Granted { expires_at, .. } if *expires_at > self.work.clock.now())
        {
            return;
        }
        if let Some(awaiting) = self.awaiting.take() {
            let Ok(keeping) = self.keeping else {
                return;
            };
            let event = FleetLoginEvent::LoggedIn {
                organization_id: awaiting.saved.organization_id.clone(),
                enrollment_id: awaiting.saved.enrollment_id.clone(),
                keeping,
            };
            self.record_ending(&event);
            awaiting.listeners.finish(event);
        }
    }
    fn start_renewal(&mut self) {
        self.renew_at = None;
        let (source, credential) = match &self.saved {
            SavedLogin::Ready {
                issuer, credential, ..
            } => (IssuerSource::Connected(issuer.clone()), credential.clone()),
            SavedLogin::Restoring { saved, credential } => (
                IssuerSource::Discover(saved.sign_in.clone()),
                credential.clone(),
            ),
            SavedLogin::Fresh | SavedLogin::RequiresApproval(_) => return,
        };
        self.renewal = Some(Box::pin(self.work.clone().refresh(
            source,
            credential,
            self.connection.clone(),
        )));
    }
    fn use_token(&mut self, refreshed: RefreshedLogin) {
        let Some(saved) = self.saved.saved().cloned() else {
            return;
        };
        self.renew_at = Some(renewal_deadline(
            self.work.clock.now(),
            refreshed.issued.valid_until,
        ));
        let enrollment_id = saved.enrollment_id.clone();
        let adopted = self.adopted.as_ref() == Some(&saved);
        self.saved = SavedLogin::Ready {
            saved,
            credential: refreshed.issued.credential,
            issuer: refreshed.issuer,
        };
        let token = if adopted {
            MachineAccessToken::Granted {
                enrollment_id,
                token: refreshed.issued.token,
                expires_at: refreshed.issued.valid_until,
            }
        } else {
            MachineAccessToken::AwaitingRoute {
                enrollment_id,
                token: refreshed.issued.token,
                expires_at: refreshed.issued.valid_until,
            }
        };
        self.token.send_replace(token);
        self.finish_adopted_login();
    }
    fn require_login(&mut self) {
        self.renew_at = None;
        self.adopted = None;
        self.saved.require_approval();
        self.token.send_replace(MachineAccessToken::NotLoggedIn);
        if let Some(awaiting) = self.awaiting.take() {
            let event = FleetLoginEvent::Refused {
                reason: FleetLoginRefusal::ConnectionUnavailable {},
            };
            self.record_ending(&event);
            awaiting.listeners.finish(event);
        }
    }
    fn refuse(&self, observer: mpsc::Sender<FleetLoginEvent>, reason: FleetLoginRefusal) {
        let event = FleetLoginEvent::Refused { reason };
        self.record_ending(&event);
        let _sent = observer.try_send(event);
    }
    fn record_ending(&self, event: &FleetLoginEvent) {
        match event {
            FleetLoginEvent::LoggedIn {
                organization_id,
                enrollment_id,
                keeping,
            } => {
                tracing::info!(event = "machineLogin", outcome = "loggedIn", organization_id = organization_id.as_str(), enrollment_id = enrollment_id.as_str(), installation = self.identity.installation.as_str(), keeping = ?keeping, "machine login ended")
            }
            FleetLoginEvent::Refused { reason } => {
                tracing::info!(event = "machineLogin", outcome = "refused", reason = ?reason, installation = self.identity.installation.as_str(), "machine login ended")
            }
            FleetLoginEvent::OutcomeUnknown { context } => {
                tracing::warn!(event = "machineLogin", outcome = "outcomeUnknown", context = ?context, installation = self.identity.installation.as_str(), "machine login ended")
            }
            FleetLoginEvent::Code { .. } => {}
        }
    }
}
fn credential_refusal(error: RefreshCredentialError) -> FleetLoginRefusal {
    use codex_router_secret_store::classified_store_open::UnusableCause;
    FleetLoginRefusal::StoreUnusable {
        cause: match error {
            RefreshCredentialError::Unusable { cause } => cause,
            RefreshCredentialError::DamagedCredential | RefreshCredentialError::WriteFailed => {
                UnusableCause::InvalidCredentialData {}
            }
        },
    }
}
enum LoginProgress {
    Code(FleetLoginEvent),
    Complete(Result<ConfirmedLogin, DeviceCodeFailure>),
}
async fn next_login_progress(login: &mut Option<PendingLogin>) -> LoginProgress {
    let Some(login) = login else {
        return pending().await;
    };
    tokio::select! {
        biased;
        Some(code) = login.codes.recv(), if login.listeners.code.is_none() => LoginProgress::Code(code),
        result = login.future.as_mut() => LoginProgress::Complete(result),
    }
}
async fn complete_renewal(
    renewal: &mut Option<LoginFuture<'static, Result<RefreshedLogin, ()>>>,
) -> Result<RefreshedLogin, ()> {
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
