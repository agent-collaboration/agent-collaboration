//! Keeps Claude's prompt cache warm while a session sits idle (D1 R16).
//!
//! One owner per Claude endpoint, fed every live event the ordered Host
//! publication owner publishes. Each session has its own warm cell on the
//! cache-warm machine (D1 state machines, design note §3.7, §11.7): cold,
//! busy with a real or a warm turn, armed, or warming. A timer armed from the
//! injected Host clock sends one warm prompt to an idle session; any refusal
//! means "not now" and waits for the next turn end. The endpoint keeps only
//! the cells' index and its timer tasks.

mod supervisor_submitter;

pub(crate) use supervisor_submitter::SupervisorWarmPromptSubmitter;

use crate::provider_settings_section::CacheWarmSetting;
use collaboration_protocol::{EndpointRef, ProviderKind, SessionId, SessionRef};
use provider_session_machines::cache_warm_machine::{
    CacheWarm, CacheWarmEffect, CacheWarmEvent, CacheWarmMachine, WarmCadence,
};
use provider_session_machines::{SessionGeneration, session_owner::OwnerAddress};
use router_clock::{ClockInstant, RuntimeClock};
use session_event_model::{InputId, SessionEvent, SessionState};
use state_machine_core::StateMachine;
use std::{
    collections::HashMap,
    future::Future,
    pin::Pin,
    sync::{
        Arc, Mutex as StdMutex,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;

/// The text of every warm prompt.
pub(crate) const WARM_PROMPT_TEXT: &str = "Cache warm: reply ok";

/// The warm prompt was not sent: the session is busy, held, uncertain or gone.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct WarmPromptRefused;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct WarmRun {
    pub(crate) owner: OwnerAddress,
    pub(crate) generation: SessionGeneration,
}

pub(crate) type WarmPromptFuture<'a> =
    Pin<Box<dyn Future<Output = Result<(), WarmPromptRefused>> + Send + 'a>>;

/// Sends the warm prompt to an idle session. It must refuse a busy session
/// and never steer, queue or cancel.
pub(crate) trait WarmPromptSubmitter: Send + Sync + 'static {
    fn submit_warm_prompt(
        &self,
        target: SessionRef,
        run: WarmRun,
        input_id: InputId,
    ) -> WarmPromptFuture<'_>;
}

pub(crate) struct ProviderCacheWarm {
    inner: Arc<WarmInner>,
}

struct WarmInner {
    endpoint: EndpointRef,
    cadence: WarmCadence,
    clock: Arc<dyn RuntimeClock>,
    submitter: Arc<dyn WarmPromptSubmitter>,
    shutdown: CancellationToken,
    stopped: AtomicBool,
    /// Each session's warm cell; the index lock covers lookup and insert.
    sessions: StdMutex<HashMap<String, Arc<StdMutex<SessionWarmCell>>>>,
    tasks: StdMutex<Vec<JoinHandle<()>>>,
}

/// One session's warm state and its armed timer.
struct SessionWarmCell {
    run: WarmRun,
    state: CacheWarm,
    timer: Option<CancellationToken>,
}

impl SessionWarmCell {
    fn disarm(&mut self) {
        if let Some(timer) = self.timer.take() {
            timer.cancel();
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
                cadence: WarmCadence {
                    every: Duration::from_secs(u64::from(setting.every_minutes) * 60),
                    keep_warm_for: Duration::from_secs(u64::from(setting.for_hours) * 3600),
                },
                clock,
                submitter,
                shutdown: CancellationToken::new(),
                stopped: AtomicBool::new(false),
                sessions: StdMutex::new(HashMap::new()),
                tasks: StdMutex::new(Vec::new()),
            }),
        }))
    }

    /// Cancels every timer and waits for its task, so nothing can submit afterwards.
    pub(crate) async fn shutdown(&self) {
        self.inner.shutdown.cancel();
        self.inner.stopped.store(true, Ordering::SeqCst);
        for cell in std::mem::take(&mut *lock(&self.inner.sessions)).into_values() {
            lock(&cell).disarm();
        }
        let tasks = std::mem::take(&mut *lock(&self.inner.tasks));
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
        .install_owner_snapshot_observer(&endpoint, warm.clone())
        .then_some(warm)
}

impl ProviderCacheWarm {
    fn observe_live_event(&self, session_id: &str, run: WarmRun, event: &SessionEvent) {
        let inner = &self.inner;
        if inner.stopped.load(Ordering::SeqCst) {
            return;
        }
        let event = match event {
            SessionEvent::InputAccepted { input_id, .. } => CacheWarmEvent::InputJoined {
                input: input_id.clone(),
            },
            SessionEvent::TurnStarted { input_id, .. } => CacheWarmEvent::TurnStarted {
                input: Some(input_id.clone()),
            },
            SessionEvent::TurnEnded { .. } => CacheWarmEvent::TurnEnded {
                at: machine_instant(inner.clock.as_ref()),
                cadence: inner.cadence,
            },
            SessionEvent::StateChanged {
                state: SessionState::Closed | SessionState::Unloaded,
            } => {
                let mut sessions = lock(&inner.sessions);
                if sessions
                    .get(session_id)
                    .is_some_and(|cell| lock(cell).run == run)
                    && let Some(cell) = sessions.remove(session_id)
                {
                    let mut cell = lock(&cell);
                    let _gone = inner.apply(&mut cell, session_id, CacheWarmEvent::SessionGone);
                }
                return;
            }
            _ => return,
        };
        let cell = inner.cell(session_id, run);
        let mut cell = lock(&cell);
        if cell.run == run {
            let _effects = inner.apply(&mut cell, session_id, event);
        }
    }
}

impl crate::session_owner_door::SessionOwnerSnapshotObserver for ProviderCacheWarm {
    fn observe(
        &self,
        session: &str,
        previous: &provider_session_machines::session_owner::SessionSnapshot,
        current: &provider_session_machines::session_owner::SessionSnapshot,
    ) {
        use provider_session_machines::session_lifecycle_machine::SessionLifecycle;

        let SessionLifecycle::Live { generation } = current.lifecycle else {
            if let SessionLifecycle::Live { generation } = previous.lifecycle {
                self.observe_live_event(
                    session,
                    WarmRun {
                        owner: previous.owner,
                        generation,
                    },
                    &SessionEvent::StateChanged {
                        state: SessionState::Unloaded,
                    },
                );
            }
            return;
        };
        let run = WarmRun {
            owner: current.owner,
            generation,
        };
        if previous.last_live_turn != current.last_live_turn
            && let Some(last) = &current.last_live_turn
        {
            if previous
                .foreground
                .as_ref()
                .is_none_or(|foreground| foreground.turn != last.turn)
            {
                self.observe_live_event(
                    session,
                    run,
                    &SessionEvent::TurnStarted {
                        turn_id: last.turn.0.to_string(),
                        input_id: last.input.clone(),
                    },
                );
            }
            self.observe_live_event(
                session,
                run,
                &SessionEvent::TurnEnded {
                    turn_id: last.turn.0.to_string(),
                    outcome: last.outcome.clone(),
                },
            );
        }
        if previous.foreground != current.foreground
            && let Some(active) = &current.foreground
        {
            self.observe_live_event(
                session,
                run,
                &SessionEvent::TurnStarted {
                    turn_id: active.turn.0.to_string(),
                    input_id: active.input.clone(),
                },
            );
        }
        if previous.recent_steer != current.recent_steer
            && let Some(steer) = &current.recent_steer
        {
            self.observe_live_event(
                session,
                run,
                &SessionEvent::InputAccepted {
                    turn_id: steer.turn.0.to_string(),
                    input_id: steer.input.clone(),
                },
            );
        }
    }
}

impl WarmInner {
    /// The session's warm cell, a cold one the first time.
    fn cell(&self, session_id: &str, run: WarmRun) -> Arc<StdMutex<SessionWarmCell>> {
        let mut sessions = lock(&self.sessions);
        let cell = sessions.entry(session_id.to_owned()).or_insert_with(|| {
            Arc::new(StdMutex::new(SessionWarmCell {
                run,
                state: CacheWarm::Cold {
                    last_real_end: None,
                },
                timer: None,
            }))
        });
        let mut current = lock(cell);
        if current.run != run {
            current.disarm();
            current.run = run;
            current.state = CacheWarm::Cold {
                last_real_end: None,
            };
        }
        Arc::clone(cell)
    }

    /// Applies one event to the session's cell under its lock and runs its
    /// timer effects; returns the warm prompt to submit, if the move asks
    /// for one. A stale event changes nothing.
    fn apply(
        self: &Arc<Self>,
        cell: &mut SessionWarmCell,
        session_id: &str,
        event: CacheWarmEvent,
    ) -> Option<InputId> {
        let Ok((next, effects)) = CacheWarmMachine::transition(&cell.state, event) else {
            return None;
        };
        cell.state = next;
        let mut submit = None;
        for effect in effects {
            match effect {
                CacheWarmEffect::ArmTimer { at } => {
                    self.arm_timer(cell, session_id, ClockInstant::ZERO.after(at.elapsed()));
                }
                CacheWarmEffect::Disarm => cell.disarm(),
                CacheWarmEffect::SubmitWarm { input } => submit = Some(input),
            }
        }
        submit
    }

    /// Arms the session's one timer for `deadline`, replacing any earlier one.
    fn arm_timer(
        self: &Arc<Self>,
        cell: &mut SessionWarmCell,
        session_id: &str,
        deadline: ClockInstant,
    ) {
        cell.disarm();
        let timer = CancellationToken::new();
        cell.timer = Some(timer.clone());
        let task = tokio::spawn(Arc::clone(self).run_timer(
            session_id.to_owned(),
            cell.run,
            timer,
            deadline,
        ));
        let mut tasks = lock(&self.tasks);
        tasks.retain(|task| !task.is_finished());
        tasks.push(task);
    }

    async fn run_timer(
        self: Arc<Self>,
        session_id: String,
        run: WarmRun,
        timer: CancellationToken,
        deadline: ClockInstant,
    ) {
        tokio::select! {
            biased;
            () = self.shutdown.cancelled() => return,
            () = timer.cancelled() => return,
            () = self.clock.sleep_until(deadline) => {}
        }
        // Decided at the moment of firing, under the session's cell lock:
        // still the armed timer, no turn running, inside the window.
        let input_id = {
            let Some(cell) = lock(&self.sessions).get(&session_id).map(Arc::clone) else {
                return;
            };
            let mut cell = lock(&cell);
            if self.stopped.load(Ordering::SeqCst) || timer.is_cancelled() || cell.run != run {
                return;
            }
            cell.timer = None;
            let event = CacheWarmEvent::TimerFired {
                input: InputId::generate(),
                at: machine_instant(self.clock.as_ref()),
                cadence: self.cadence,
            };
            match self.apply(&mut cell, &session_id, event) {
                Some(input_id) => input_id,
                None => return,
            }
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
            submitted = self.submitter.submit_warm_prompt(target, run, input_id) => submitted,
        };
        if submitted.is_err() {
            // Not now: nothing was sent, so the next real turn end re-arms.
            if let Some(cell) = lock(&self.sessions).get(&session_id).map(Arc::clone) {
                let mut cell = lock(&cell);
                if cell.run == run {
                    let _refused = self.apply(&mut cell, &session_id, CacheWarmEvent::WarmRefused);
                }
            }
        }
    }
}

fn lock<T>(mutex: &StdMutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// The injected clock's now, as the cache-warm machine receives it: the
/// time elapsed since the clock's origin.
fn machine_instant(clock: &dyn RuntimeClock) -> provider_session_machines::MonotonicInstant {
    provider_session_machines::MonotonicInstant::from_elapsed(
        clock
            .monotonic_now()
            .saturating_duration_since(ClockInstant::ZERO),
    )
}

#[cfg(test)]
#[path = "provider_cache_warm_test.rs"]
mod tests;
