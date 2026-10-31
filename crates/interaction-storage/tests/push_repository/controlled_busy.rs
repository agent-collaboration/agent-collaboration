//! Drive the retry clock only after a real held-lock Busy result.
use std::{error::Error, future::Future, time::Duration};
use tokio::sync::watch;
use tracing::field::{Field, Visit};
use tracing_subscriber::{Layer, layer::Context, prelude::*};

struct PushField<'push> {
    expected: &'push str,
    matched: bool,
}

impl Visit for PushField<'_> {
    fn record_str(&mut self, field: &Field, value: &str) {
        if field.name() == "push_id" && value == self.expected {
            self.matched = true;
        }
    }
    fn record_debug(&mut self, _field: &Field, _value: &dyn std::fmt::Debug) {}
}

struct NativeBusyLayer {
    expected: String,
    busy: watch::Sender<bool>,
}

impl<Subscriber: tracing::Subscriber> Layer<Subscriber> for NativeBusyLayer {
    fn on_event(&self, event: &tracing::Event<'_>, _context: Context<'_, Subscriber>) {
        if event.metadata().target() != "interaction_storage::runtime_test_push_insert_busy" {
            return;
        }
        let mut push = PushField {
            expected: &self.expected,
            matched: false,
        };
        event.record(&mut push);
        if push.matched {
            self.busy.send_replace(true);
        }
    }
}

pub(super) fn observe_push(push_id: &str) -> (impl tracing::Subscriber, watch::Receiver<bool>) {
    let (busy, observed) = watch::channel(false);
    (
        tracing_subscriber::registry().with(NativeBusyLayer {
            expected: push_id.to_owned(),
            busy,
        }),
        observed,
    )
}

pub(super) async fn complete_after_actual_busy<TOutput>(
    mut busy: watch::Receiver<bool>,
    inserting: impl Future<Output = TOutput>,
) -> Result<TOutput, Box<dyn Error>> {
    // Preserve the original six-second failure guard in real time so driving
    // the retry clock cannot consume it or mask the native driver's busy wait.
    let (release, released) = std::sync::mpsc::channel();
    let mut bound =
        tokio::task::spawn_blocking(move || released.recv_timeout(Duration::from_secs(6)));
    tokio::pin!(inserting);
    let mut advanced = false;
    let mut bound_joined = false;
    let result = loop {
        tokio::select! {
            observed = busy.wait_for(|seen| *seen), if !advanced => {
                if let Err(error) = observed {
                    break Err(Box::<dyn Error>::from(error));
                }
                tokio::time::pause();
                tokio::time::advance(Duration::from_millis(5001)).await;
                tokio::time::resume();
                advanced = true;
            }
            completed = &mut inserting => break Ok(completed),
            completed = &mut bound => {
                bound_joined = true;
                break match completed {
                    Ok(Err(error)) => Err(Box::<dyn Error>::from(error)),
                    Err(error) => Err(Box::<dyn Error>::from(error)),
                    Ok(Ok(())) => Err("busy bound released before insertion settled".into()),
                };
            }
        }
    };
    let _released = release.send(());
    if !bound_joined {
        bound.await??;
    }
    if !advanced {
        return Err("insertion settled without its actual native Busy result".into());
    }
    result
}
