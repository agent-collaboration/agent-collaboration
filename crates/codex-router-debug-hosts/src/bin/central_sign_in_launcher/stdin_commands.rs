//! One owned reader polls stdin and an owned cancellation pipe. No blocking
//! line read or Tokio stdin task survives launcher shutdown.
use super::launcher_failure::{LauncherFailure, LauncherResult};
use rustix::{
    event::{PollFd, PollFlags, poll},
    fd::OwnedFd,
    io::{Errno, FdFlags},
};
use std::{io, thread::JoinHandle};
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender, unbounded_channel};

pub(super) struct StdinCommands {
    cancellation_writer: Option<OwnedFd>,
    reader: Option<JoinHandle<io::Result<()>>>,
    lines: UnboundedReceiver<Vec<u8>>,
}
impl StdinCommands {
    pub(super) fn start() -> LauncherResult<Self> {
        let stdin = rustix::io::fcntl_dupfd_cloexec(rustix::stdio::stdin(), 0)
            .map_err(|_| LauncherFailure::StartReader)?;
        // pipe_with is unavailable on Apple; all three descriptors remain owned.
        let (cancel_read, cancel_write) =
            rustix::pipe::pipe().map_err(|_| LauncherFailure::StartReader)?;
        rustix::io::fcntl_setfd(&cancel_read, FdFlags::CLOEXEC)
            .map_err(|_| LauncherFailure::StartReader)?;
        rustix::io::fcntl_setfd(&cancel_write, FdFlags::CLOEXEC)
            .map_err(|_| LauncherFailure::StartReader)?;
        let (sender, lines) = unbounded_channel();
        let reader = std::thread::Builder::new()
            .name("signin-launcher-stdin".into())
            .spawn(move || read_commands(stdin, cancel_read, sender))
            .map_err(|_| LauncherFailure::StartReader)?;
        Ok(Self {
            cancellation_writer: Some(cancel_write),
            reader: Some(reader),
            lines,
        })
    }
    pub(super) async fn next_line(&mut self) -> Option<Vec<u8>> {
        self.lines.recv().await
    }
    pub(super) async fn shutdown(mut self) -> LauncherResult {
        // Closing the only writer wakes poll with HUP, without a write that
        // could block or encounter SIGPIPE after stdin has already reached EOF.
        drop(self.cancellation_writer.take());
        let Some(reader) = self.reader.take() else {
            return Ok(());
        };
        tokio::task::spawn_blocking(move || reader.join())
            .await
            .map_err(|_| LauncherFailure::JoinReader)?
            .map_err(|_| LauncherFailure::JoinReader)?
            .map_err(|_| LauncherFailure::ReadCommands)
    }
}
impl Drop for StdinCommands {
    fn drop(&mut self) {
        drop(self.cancellation_writer.take());
        // Error paths retain the same cancellation-and-join ownership.
        if let Some(reader) = self.reader.take() {
            let _ = reader.join();
        }
    }
}

fn read_commands(
    stdin: OwnedFd,
    cancel_read: OwnedFd,
    sender: UnboundedSender<Vec<u8>>,
) -> io::Result<()> {
    let mut pending = Vec::new();
    let mut buffer = [0_u8; 4096];
    loop {
        let mut watched = [
            PollFd::new(&stdin, PollFlags::IN),
            PollFd::new(&cancel_read, PollFlags::IN),
        ];
        match poll(&mut watched, None) {
            Ok(_) => {}
            Err(Errno::INTR) => continue,
            Err(error) => return Err(error.into()),
        }
        let [input, cancellation] = &watched;
        if cancellation
            .revents()
            .intersects(PollFlags::IN | PollFlags::HUP | PollFlags::ERR | PollFlags::NVAL)
        {
            return Ok(());
        }
        if input.revents().intersects(PollFlags::ERR | PollFlags::NVAL) {
            return Err(io::Error::other("stdin descriptor unavailable"));
        }
        if !input.revents().intersects(PollFlags::IN | PollFlags::HUP) {
            continue;
        }
        let read = match rustix::io::read(&stdin, buffer.as_mut_slice()) {
            Ok(read) => read,
            Err(Errno::INTR | Errno::AGAIN) => continue,
            Err(error) => return Err(error.into()),
        };
        if read == 0 {
            if !pending.is_empty() {
                let _ = sender.send(pending);
            }
            return Ok(());
        }
        pending.extend_from_slice(
            buffer
                .get(..read)
                .ok_or_else(|| io::Error::other("stdin read exceeded its buffer"))?,
        );
        while let Some(newline) = pending.iter().position(|byte| *byte == b'\n') {
            let following = pending.split_off(newline + 1);
            let mut line = std::mem::replace(&mut pending, following);
            let _ = line.pop();
            if sender.send(line).is_err() {
                return Ok(());
            }
        }
    }
}
