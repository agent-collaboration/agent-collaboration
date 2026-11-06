//! One frozen mail clock and a private, causally acknowledged debug control stream.
use chrono::{DateTime, Utc};
use collaboration_service::{MailDeliveryRuntime, SubscriptionClock};
use std::{io, path::Path, sync::Arc};
use tokio::{
    io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader},
    net::UnixStream,
    sync::watch,
    time::Instant,
};
use tokio_util::sync::CancellationToken;

const INPUT_FILE: &str = "messaging-test-mail-clock.json";
const MAX_FRAME_BYTES: u64 = 4096;

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ClockInput {
    now: DateTime<Utc>,
    control_socket: std::path::PathBuf,
}

#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct ClockStep {
    now: DateTime<Utc>,
}

#[derive(serde::Serialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
enum ClockReply {
    Ready { now: DateTime<Utc> },
    Stepped { now: DateTime<Utc> },
}

#[derive(Clone, Copy)]
struct ClockReading {
    wall: DateTime<Utc>,
    monotonic: Instant,
}

struct SteppedMailClock {
    reading: watch::Sender<ClockReading>,
}

impl SteppedMailClock {
    fn new(now: DateTime<Utc>) -> Self {
        Self {
            reading: watch::channel(ClockReading {
                wall: now,
                monotonic: Instant::now(),
            })
            .0,
        }
    }

    fn advance_to(&self, now: DateTime<Utc>) -> io::Result<()> {
        let previous = *self.reading.borrow();
        let elapsed = (now - previous.wall)
            .to_std()
            .map_err(|_| io::Error::other("debug mail clock cannot move backwards"))?;
        let monotonic = previous
            .monotonic
            .checked_add(elapsed)
            .ok_or_else(|| io::Error::other("debug mail clock step exceeds monotonic range"))?;
        self.reading.send_replace(ClockReading {
            wall: now,
            monotonic,
        });
        Ok(())
    }
}

impl SubscriptionClock for SteppedMailClock {
    fn now(&self) -> DateTime<Utc> {
        self.reading.borrow().wall
    }

    fn monotonic_now(&self) -> Instant {
        self.reading.borrow().monotonic
    }

    fn sleep_until(
        &self,
        deadline: Instant,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = ()> + Send + '_>> {
        Box::pin(async move {
            let mut reading = self.reading.subscribe();
            loop {
                if reading.borrow_and_update().monotonic >= deadline {
                    return;
                }
                if reading.changed().await.is_err() {
                    return;
                }
            }
        })
    }
}

pub(crate) struct PreparedMailClock {
    clock: Arc<SteppedMailClock>,
    control: BufReader<UnixStream>,
}

impl PreparedMailClock {
    pub(crate) async fn load(directory: &Path) -> io::Result<Option<Self>> {
        use std::os::unix::fs::{MetadataExt, PermissionsExt};
        let path = directory.join(INPUT_FILE);
        let file = match std::fs::symlink_metadata(&path) {
            Ok(file) => file,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(error),
        };
        let parent = std::fs::symlink_metadata(directory)?;
        let owner = rustix::process::geteuid().as_raw();
        if !directory.is_absolute()
            || !parent.is_dir()
            || parent.uid() != owner
            || parent.permissions().mode() & 0o077 != 0
            || !file.is_file()
            || file.uid() != owner
            || file.permissions().mode() & 0o077 != 0
            || file.len() > MAX_FRAME_BYTES
        {
            return Err(io::Error::other(
                "debug mail clock requires a private owner-controlled fixture file",
            ));
        }
        let input: ClockInput =
            serde_json::from_slice(&std::fs::read(path)?).map_err(io::Error::other)?;
        crate::debug_runtime_barrier::validate_barrier_socket(&input.control_socket)?;
        let control = UnixStream::connect(input.control_socket).await?;
        Ok(Some(Self {
            clock: Arc::new(SteppedMailClock::new(input.now)),
            control: BufReader::new(control),
        }))
    }

    pub(crate) fn clock(&self) -> Arc<dyn SubscriptionClock> {
        self.clock.clone()
    }

    pub(crate) async fn run(
        mut self,
        mail: Arc<MailDeliveryRuntime>,
        shutdown: CancellationToken,
    ) -> io::Result<()> {
        tokio::select! {
            result = self.serve(mail) => result,
            () = shutdown.cancelled() => Ok(()),
        }
    }

    async fn reply(&mut self, reply: ClockReply) -> io::Result<()> {
        let mut frame = serde_json::to_vec(&reply).map_err(io::Error::other)?;
        frame.push(b'\n');
        self.control.get_mut().write_all(&frame).await
    }

    async fn serve(&mut self, mail: Arc<MailDeliveryRuntime>) -> io::Result<()> {
        self.reply(ClockReply::Ready {
            now: self.clock.now(),
        })
        .await?;
        loop {
            let mut frame = Vec::new();
            let read = (&mut self.control)
                .take(MAX_FRAME_BYTES + 1)
                .read_until(b'\n', &mut frame)
                .await?;
            if read == 0 {
                return Ok(());
            }
            if u64::try_from(read).map_err(io::Error::other)? > MAX_FRAME_BYTES
                || frame.last() != Some(&b'\n')
            {
                return Err(io::Error::other(
                    "debug mail clock requires a bounded newline-delimited frame",
                ));
            }
            let step: ClockStep = serde_json::from_slice(&frame).map_err(io::Error::other)?;
            self.clock.advance_to(step.now)?;
            mail.runtime_test_run_due_record_removals()
                .await
                .map_err(io::Error::other)?;
            self.reply(ClockReply::Stepped {
                now: self.clock.now(),
            })
            .await?;
        }
    }
}

#[cfg(test)]
#[path = "debug_mail_clock_test.rs"]
mod tests;
