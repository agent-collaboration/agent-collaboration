//! PTY output is the render checkpoint for the real pinned TUI journeys.
use std::{
    io,
    sync::{Arc, Mutex},
    time::Duration,
};
use tokio::{io::AsyncReadExt, process::ChildStdout, sync::watch, task::JoinHandle};

pub(super) struct TuiOutput {
    bytes: Arc<Mutex<Vec<u8>>>,
    changed: watch::Receiver<()>,
    reader: JoinHandle<io::Result<()>>,
}

impl TuiOutput {
    pub(super) fn capture(mut output: ChildStdout) -> Self {
        let bytes = Arc::new(Mutex::new(Vec::new()));
        let captured = bytes.clone();
        let (changed, observed) = watch::channel(());
        let reader = tokio::spawn(async move {
            let mut chunk = [0_u8; 4096];
            loop {
                let read = output.read(&mut chunk).await?;
                if read == 0 {
                    return Ok(());
                }
                captured
                    .lock()
                    .map_err(|_| io::Error::other("TUI output lock poisoned"))?
                    .extend_from_slice(&chunk[..read]);
                changed.send_replace(());
            }
        });
        Self {
            bytes,
            changed: observed,
            reader,
        }
    }

    pub(super) async fn wait_for_text(
        &mut self,
        expected: &str,
        bound: Duration,
    ) -> io::Result<()> {
        tokio::time::timeout(bound, async {
            loop {
                self.changed.borrow_and_update();
                let found = {
                    let bytes = self
                        .bytes
                        .lock()
                        .map_err(|_| io::Error::other("TUI output lock poisoned"))?;
                    let text = String::from_utf8_lossy(&bytes);
                    // A word can arrive midway through a PTY read/render. The
                    // pinned TUI closes each synchronized frame with this marker.
                    text.find(expected)
                        .and_then(|start| text.get(start + expected.len()..))
                        .is_some_and(|remaining| remaining.contains("\x1b[?2026l"))
                };
                if found {
                    return Ok(());
                }
                self.changed.changed().await.map_err(|_| {
                    io::Error::other(format!("TUI exited before rendering {expected}"))
                })?;
            }
        })
        .await
        .map_err(|_| io::Error::other(format!("TUI did not render {expected} before its bound")))?
    }

    pub(super) async fn finish(self) -> io::Result<String> {
        self.reader.await.map_err(io::Error::other)??;
        String::from_utf8(
            self.bytes
                .lock()
                .map_err(|_| io::Error::other("TUI output lock poisoned"))?
                .clone(),
        )
        .map_err(io::Error::other)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use futures_util::poll;
    use std::process::Stdio;
    use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

    #[tokio::test]
    async fn a_text_prefix_does_not_release_input_before_its_frame_finishes() {
        let mut producer = tokio::process::Command::new("/bin/sh")
            .args(["-c", "printf 'Trust and continue'; printf 'partial\\n' >&2; read allow_frame; printf '\\033[?2026l'"])
            .stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped())
            .kill_on_drop(true).spawn().expect("owned frame producer");
        let mut output = TuiOutput::capture(producer.stdout.take().expect("frame output"));
        let mut acknowledgement = String::new();
        BufReader::new(producer.stderr.take().expect("producer acknowledgement"))
            .read_line(&mut acknowledgement)
            .await
            .expect("partial frame written");
        assert_eq!(acknowledgement, "partial\n");
        output
            .changed
            .changed()
            .await
            .expect("partial frame captured");
        {
            let observed = output.wait_for_text("Trust and continue", Duration::from_secs(5));
            tokio::pin!(observed);
            assert!(
                poll!(observed.as_mut()).is_pending(),
                "partial frame released input"
            );
            producer
                .stdin
                .as_mut()
                .expect("frame release")
                .write_all(b"release\n")
                .await
                .expect("release complete frame");
            observed.await.expect("completed frame rendered");
        }
        assert!(
            producer
                .wait()
                .await
                .expect("join frame producer")
                .success()
        );
        assert!(
            output
                .finish()
                .await
                .expect("drain frame producer")
                .contains("Trust and continue")
        );
    }
}
