//! Starts the Router's central link with the same machine id used by the Host.
//! Until the Router has a token source, immediate calls report not enrolled
//! and outbox writes wait in the local store.
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
use workspace_ids::MachineId;

/// A link the Host has configured: B3's token source and the link transport.
pub struct ConfiguredCentralLink {
    pub tokens: Arc<dyn MachineTokenSource>,
    pub transport: Arc<dyn LinkTransport>,
}

/// Opens the Router's machine_local.sqlite in `state_directory` and starts
/// the link on it. None when the store does not open (the link is then not
/// started; the rest of the Router runs).
pub async fn open_installation_link(
    state_directory: &std::path::Path,
    machine_id: &MachineId,
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
    Some(start_installation_link(
        machine_id, clock, store, configured, tasks, stop,
    ))
}

/// Starts the link driver with the Host machine identifier.
pub fn start_installation_link(
    machine_id: &MachineId,
    clock: Arc<dyn RuntimeClock>,
    store: Arc<Mutex<dyn OutboxStorage>>,
    configured: Option<ConfiguredCentralLink>,
    tasks: &TaskTracker,
    stop: CancellationToken,
) -> RouterLinkCalls {
    let ConfiguredCentralLink { tokens, transport } =
        configured.unwrap_or_else(|| ConfiguredCentralLink {
            tokens: Arc::new(NotEnrolledTokens),
            transport: Arc::new(UnconfiguredLinkTransport),
        });
    let parts = CentralLinkParts {
        jitter: Arc::new(InstallationFlushJitter::new(machine_id)),
        installation: machine_id.clone(),
        clock,
        store,
        transport,
        tokens,
        outcomes: Arc::new(LoggedFlushOutcomes),
    };
    RouterLinkCalls::start(parts, tasks, stop)
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
