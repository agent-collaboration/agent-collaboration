//! Keeps Claude's prompt cache warm while a session sits idle (D1 R16).
//!
//! One owner per Claude endpoint, fed every live event the ordered Host
//! publication owner publishes. It keeps, per session and in memory only, what
//! it has seen: whether a turn is running, whether the current turn is real
//! work, when the last real turn ended, and which input IDs it sent itself.
//! A timer armed from the injected Host clock sends one warm prompt to an idle
//! session; any refusal means "not now" and waits for the next turn end.

mod supervisor_submitter;

pub(crate) use supervisor_submitter::SupervisorWarmPromptSubmitter;

use crate::provider_session_event_sink::LiveSessionEventObserver;
use crate::provider_settings_section::CacheWarmSetting;
use collaboration_protocol::{EndpointRef, ProviderKind, SessionId, SessionRef};
use router_clock::{ClockInstant, RuntimeClock};
use session_event_model::{InputId, SessionEvent, SessionState};
use std::{
    collections::{HashMap, HashSet},
    future::Future,
    pin::Pin,
    sync::{Arc, Mutex as StdMutex},
    time::Duration,
};
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;

/// The text of every warm prompt.
pub(crate) const WARM_PROMPT_TEXT: &str = "Cache warm: reply ok";

/// The warm prompt was not sent: the session is busy, held, uncertain or gone.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct WarmPromptRefused;

pub(crate) type WarmPromptFuture<'a> =
    Pin<Box<dyn Future<Output = Result<(), WarmPromptRefused>> + Send + 'a>>;

/// Sends the warm prompt to an idle session. It must refuse a busy session
/// and never steer, queue or cancel.
pub(crate) trait WarmPromptSubmitter: Send + Sync + 'static {
    fn submit_warm_prompt(&self, target: SessionRef, input_id: InputId) -> WarmPromptFuture<'_>;
}

pub(crate) struct ProviderCacheWarm {
    inner: Arc<WarmInner>,
}

struct WarmInner {
    endpoint: EndpointRef,
    every: Duration,
    keep_warm_for: Duration,
    clock: Arc<dyn RuntimeClock>,
    submitter: Arc<dyn WarmPromptSubmitter>,
    shutdown: CancellationToken,
    state: StdMutex<WarmState>,
}

#[derive(Default)]
struct WarmState {
    sessions: HashMap<String, SessionWarmState>,
    tasks: Vec<JoinHandle<()>>,
    stopped: bool,
}

#[derive(Default)]
struct SessionWarmState {
    /// Input IDs this module sent, so its own turns are not real work.
    warm_inputs: HashSet<InputId>,
    turn_running: bool,
    current_turn_is_real: bool,
    last_real_turn_end: Option<ClockInstant>,
    timer: Option<CancellationToken>,
}

impl SessionWarmState {
    fn disarm(&mut self) {
        if let Some(timer) = self.timer.take() {
            timer.cancel();
        }
    }

    fn mark_real_unless_warm(&mut self, input_id: &InputId) {
        if !self.warm_inputs.contains(input_id) {
            self.current_turn_is_real = true;
        }
    }
}

impl ProviderCacheWarm {
    /// A module only for a Claude endpoint whose setting is on; Cursor and
    /// Codex sessions, and a setting that is off, get none.
    pub(crate) fn for_provider(
        provider: ProviderKind,
        setting: Option<CacheWarmSetting>,
        endpoint: EndpointRef,
        clock: Arc<dyn RuntimeClock>,
        submitter: Arc<dyn WarmPromptSubmitter>,
    ) -> Option<Arc<Self>> {
        let setting = setting.filter(|setting| setting.enabled)?;
        if provider != ProviderKind::ClaudeCode {
            return None;
        }
        Some(Arc::new(Self {
            inner: Arc::new(WarmInner {
                endpoint,
                every: Duration::from_secs(u64::from(setting.every_minutes) * 60),
                keep_warm_for: Duration::from_secs(u64::from(setting.for_hours) * 3600),
                clock,
                submitter,
                shutdown: CancellationToken::new(),
                state: StdMutex::new(WarmState::default()),
            }),
        }))
    }

    /// Cancels every timer and waits for its task, so nothing can submit afterwards.
    pub(crate) async fn shutdown(&self) {
        self.inner.shutdown.cancel();
        let tasks = {
            let mut state = self.inner.lock_state();
            state.stopped = true;
            state.sessions.clear();
            std::mem::take(&mut state.tasks)
        };
        for task in tasks {
            let _ = task.await;
        }
    }
}

/// The Host's one cache-warm owner for the started Claude endpoint, listening
/// to that endpoint's live events and sending through the supervisor. `None`
/// when the endpoint did not start, the setting is off, or no storage exists.
pub(crate) fn compose_provider_cache_warm(
    configured: Option<(EndpointRef, CacheWarmSetting)>,
    supervisor: Option<&Arc<crate::ExternalProviderSupervisor>>,
    store: Option<&Arc<tokio::sync::Mutex<collaboration_runtime::ProviderOperationStore>>>,
    clock: Arc<dyn RuntimeClock>,
) -> Option<Arc<ProviderCacheWarm>> {
    let (endpoint, setting) = configured?;
    let supervisor = supervisor?;
    let submitter = Arc::new(SupervisorWarmPromptSubmitter::new(
        Arc::clone(supervisor),
        Arc::clone(store?),
    ));
    let warm = ProviderCacheWarm::for_provider(
        ProviderKind::ClaudeCode,
        Some(setting),
        endpoint.clone(),
        clock,
        submitter,
    )?;
    supervisor
        .install_live_event_observer(&endpoint, warm.clone())
        .then_some(warm)
}

impl LiveSessionEventObserver for ProviderCacheWarm {
    fn observe_live_event(&self, session_id: &str, event: &SessionEvent) {
        let inner = &self.inner;
        let mut state = inner.lock_state();
        if state.stopped {
            return;
        }
        match event {
            SessionEvent::InputAccepted { input_id, .. } => {
                state
                    .sessions
                    .entry(session_id.to_owned())
                    .or_default()
                    .mark_real_unless_warm(input_id);
            }
            SessionEvent::TurnStarted { input_id, .. } => {
                let session = state.sessions.entry(session_id.to_owned()).or_default();
                session.disarm();
                session.turn_running = true;
                session.mark_real_unless_warm(input_id);
            }
            SessionEvent::TurnEnded { .. } => {
                let now = inner.clock.monotonic_now();
                let session = state.sessions.entry(session_id.to_owned()).or_default();
                session.turn_running = false;
                if session.current_turn_is_real {
                    session.last_real_turn_end = Some(now);
                    session.warm_inputs.clear();
                }
                session.current_turn_is_real = false;
                inner.arm_timer(&mut state, session_id, now);
            }
            SessionEvent::StateChanged {
                state: SessionState::Closed | SessionState::Unloaded,
            } => {
                if let Some(mut session) = state.sessions.remove(session_id) {
                    session.disarm();
                }
            }
            _ => {}
        }
    }
}

impl WarmInner {
    fn lock_state(&self) -> std::sync::MutexGuard<'_, WarmState> {
        self.state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    /// Arms the one timer for a session whose turn just ended, if the last real
    /// turn is still inside the keep-warm window.
    fn arm_timer(self: &Arc<Self>, state: &mut WarmState, session_id: &str, now: ClockInstant) {
        let Some(session) = state.sessions.get_mut(session_id) else {
            return;
        };
        session.disarm();
        let Some(last_real) = session.last_real_turn_end else {
            return;
        };
        if now.saturating_duration_since(last_real) >= self.keep_warm_for {
            return;
        }
        let timer = CancellationToken::new();
        session.timer = Some(timer.clone());
        let task = tokio::spawn(Arc::clone(self).run_timer(
            session_id.to_owned(),
            timer,
            now + self.every,
        ));
        state.tasks.retain(|task| !task.is_finished());
        state.tasks.push(task);
    }

    async fn run_timer(
        self: Arc<Self>,
        session_id: String,
        timer: CancellationToken,
        deadline: ClockInstant,
    ) {
        tokio::select! {
            biased;
            () = self.shutdown.cancelled() => return,
            () = timer.cancelled() => return,
            () = self.clock.sleep_until(deadline) => {}
        }
        let Some(input_id) = self.begin_warm_turn(&session_id, &timer) else {
            return;
        };
        let Ok(session_id_value) = SessionId::try_from(session_id.clone()) else {
            return;
        };
        let target = SessionRef {
            endpoint: self.endpoint.clone(),
            session_id: session_id_value,
        };
        let submitted = tokio::select! {
            biased;
            () = self.shutdown.cancelled() => return,
            submitted = self.submitter.submit_warm_prompt(target, input_id.clone()) => submitted,
        };
        if submitted.is_err() {
            // Not now: nothing was sent, so the next real turn end re-arms.
            let mut state = self.lock_state();
            if let Some(session) = state.sessions.get_mut(&session_id) {
                session.warm_inputs.remove(&input_id);
            }
        }
    }

    /// Decides at the moment of firing, under the same lock as every event:
    /// the timer is still the armed one, no turn is running, and the last real
    /// turn ended inside the window. Records the warm input ID before it is sent.
    fn begin_warm_turn(&self, session_id: &str, timer: &CancellationToken) -> Option<InputId> {
        let mut state = self.lock_state();
        if state.stopped || timer.is_cancelled() {
            return None;
        }
        let session = state.sessions.get_mut(session_id)?;
        session.timer = None;
        let last_real = session.last_real_turn_end?;
        if session.turn_running
            || self
                .clock
                .monotonic_now()
                .saturating_duration_since(last_real)
                >= self.keep_warm_for
        {
            return None;
        }
        let input_id = InputId::generate();
        session.warm_inputs.insert(input_id.clone());
        Some(input_id)
    }
}

#[cfg(test)]
#[path = "provider_cache_warm_test.rs"]
mod tests;
