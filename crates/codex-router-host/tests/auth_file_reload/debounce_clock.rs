//! Advance only debounce deadlines actually armed by this in-process Host.
use std::{error::Error, future::Future, time::Duration};
use tokio::sync::watch;
use tracing_subscriber::{Layer, layer::Context, prelude::*};

struct ArmedDebounceLayer(watch::Sender<u64>);

impl<Subscriber: tracing::Subscriber> Layer<Subscriber> for ArmedDebounceLayer {
    fn on_event(&self, event: &tracing::Event<'_>, _context: Context<'_, Subscriber>) {
        if event.metadata().target() == "codex_router_host::runtime_test_auth_debounce" {
            self.0.send_modify(|count| *count = count.saturating_add(1));
        }
    }
}

pub(super) struct DebounceClock(watch::Receiver<u64>);

pub(super) fn observe_armed_deadlines() -> (impl tracing::Subscriber, DebounceClock) {
    let (armed, observed) = watch::channel(0);
    (
        tracing_subscriber::registry().with(ArmedDebounceLayer(armed)),
        DebounceClock(observed),
    )
}

impl DebounceClock {
    pub(super) fn before_write(&mut self) {
        self.0.borrow_and_update();
    }

    pub(super) async fn complete(
        &mut self,
        completed_reload: impl Future<Output = Result<(), Box<dyn Error>>>,
    ) -> Result<(), Box<dyn Error>> {
        // Keep native filesystem/process IO from auto-advancing paused timers.
        // This is the same ten-second failure bound as the completed-reload wait.
        let (release, released) = std::sync::mpsc::channel();
        let clock_guard =
            tokio::task::spawn_blocking(move || released.recv_timeout(Duration::from_secs(10)));
        tokio::pin!(completed_reload);
        let mut armed_observed = false;
        let result = loop {
            tokio::select! {
                armed = self.0.changed() => {
                    if let Err(error) = armed {
                        break Err(Box::<dyn Error>::from(error));
                    }
                    self.0.borrow_and_update();
                    armed_observed = true;
                    tokio::time::pause();
                    tokio::time::advance(Duration::from_millis(251)).await;
                    // Child protocol and filesystem IO always run with time resumed.
                    tokio::time::resume();
                }
                completed = &mut completed_reload => break completed,
            }
        };
        let _released = release.send(());
        clock_guard.await??;
        result?;
        if !armed_observed {
            return Err("reload completed without its actual armed debounce deadline".into());
        }
        Ok(())
    }
}
