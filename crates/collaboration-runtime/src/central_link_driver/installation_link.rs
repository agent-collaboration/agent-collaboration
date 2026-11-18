//! Starts the Router's central link for this installation. The link's machine
//! id is the installation's persisted peer UUIDv7 (B3 contract; a v4 Router
//! is re-keyed at enrollment). Stand-in: until B3's Router token source lands
//! the Host configures no link, and the driver runs with a token source that
//! answers NeedsNewCode: the link reports "not enrolled; new code needed",
//! immediate calls answer NotEnrolled, and outbox writes are stored and wait.
use super::{
    CentralLinkParts, InstallationFlushJitter, LinkSocket, LinkTransport, LinkTransportError,
    MachineTokenSource, RouterLinkCalls, TokenAvailability, TransportFuture,
};
use crate::{MachineLocalStore, OutboxStorage};
use central_link_client::{FlushOutcomeSink, FlushOutcomes};
use central_link_protocol::{MachineTokenText, TokenRenewalResult};
use router_clock::RuntimeClock;
use std::sync::Arc;
use tokio::sync::Mutex;
use tokio_util::{sync::CancellationToken, task::TaskTracker};
use workspace_model::MachineId;

/// A link the Host has configured: B3's token source and the link transport.
pub struct ConfiguredCentralLink {
    pub tokens: Arc<dyn MachineTokenSource>,
    pub transport: Arc<dyn LinkTransport>,
}

/// Opens the Router's machine_local.sqlite in `state_directory` and starts
/// the link on it. None when the store does not open (the link is then not
/// started; the rest of the Router runs) or the peer is not a machine id.
pub async fn open_installation_link(
    state_directory: &std::path::Path,
    installation: &fleet_sync_protocol::PeerId,
    clock: Arc<dyn RuntimeClock>,
    configured: Option<ConfiguredCentralLink>,
    tasks: &TaskTracker,
    stop: CancellationToken,
) -> Option<RouterLinkCalls> {
    let path = state_directory.join(crate::MACHINE_LOCAL_DATABASE);
    let store = match MachineLocalStore::open(&path).await {
        Ok(store) => Arc::new(Mutex::new(crate::MachineLocalOutbox::new(store)))
            as Arc<Mutex<dyn OutboxStorage>>,
        Err(error) => {
            tracing::warn!(%error, "central link not started: machine-local storage unavailable");
            return None;
        }
    };
    start_installation_link(installation, clock, store, configured, tasks, stop)
}

/// Starts the link driver for `installation`. None when the peer is not a
/// machine id; startup refuses a corrupt peer file before this point.
pub fn start_installation_link(
    installation: &fleet_sync_protocol::PeerId,
    clock: Arc<dyn RuntimeClock>,
    store: Arc<Mutex<dyn OutboxStorage>>,
    configured: Option<ConfiguredCentralLink>,
    tasks: &TaskTracker,
    stop: CancellationToken,
) -> Option<RouterLinkCalls> {
    let Ok(machine) = installation.to_string().parse::<MachineId>() else {
        tracing::warn!("central link not started: the installation peer is not a machine id");
        return None;
    };
    let ConfiguredCentralLink { tokens, transport } =
        configured.unwrap_or_else(|| ConfiguredCentralLink {
            tokens: Arc::new(NotEnrolledTokens),
            transport: Arc::new(UnconfiguredLinkTransport),
        });
    let parts = CentralLinkParts {
        jitter: Arc::new(InstallationFlushJitter::new(&machine)),
        installation: machine,
        clock,
        store,
        transport,
        tokens,
        outcomes: Arc::new(LoggedFlushOutcomes),
    };
    Some(RouterLinkCalls::start(parts, tasks, stop))
}

/// No sign-in token on this Router: not enrolled until a source is configured.
pub struct NotEnrolledTokens;

impl MachineTokenSource for NotEnrolledTokens {
    fn availability(&self) -> TokenAvailability {
        TokenAvailability::NeedsNewCode
    }
    fn changed(&self) -> TransportFuture<'_, ()> {
        Box::pin(std::future::pending())
    }
    fn report_renewal(&self, _outcome: &TokenRenewalResult) {}
}

/// No link URL configured; never reached while the tokens are NeedsNewCode.
pub struct UnconfiguredLinkTransport;

impl LinkTransport for UnconfiguredLinkTransport {
    fn connect<'call>(
        &'call self,
        _token: &'call MachineTokenText,
    ) -> TransportFuture<'call, Result<Box<dyn LinkSocket>, LinkTransportError>> {
        Box::pin(async { Err(LinkTransportError::Connect) })
    }
}

/// Stand-in for the CLI+MCP side's per-session receipts (PR 4): records each
/// flush's counts, never the content.
struct LoggedFlushOutcomes;

impl FlushOutcomeSink for LoggedFlushOutcomes {
    fn deliver(&self, outcomes: FlushOutcomes) {
        tracing::info!(
            event = "centralLink.flushOutcomes",
            flush_at = outcomes.flush_at.unix_millis(),
            answered = outcomes.outcomes.len(),
            complete = outcomes.complete,
        );
    }
    fn ended(&self, ended: Vec<central_link_client::EndedWrite>) {
        for write in ended {
            tracing::info!(
                event = "centralLink.writeEnded",
                request_id = write.request_id.as_str(),
                reason = ?write.reason,
            );
        }
    }
}

#[cfg(test)]
#[path = "installation_link_test.rs"]
mod tests;
