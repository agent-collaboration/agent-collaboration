//! Bounded kernel process-exit waits for a connected operator peer, without polling timers.

use std::io;
use std::time::Instant;

pub(super) struct ProcessExitObserver {
    #[cfg(target_os = "macos")]
    queue: nix::sys::event::Kqueue,
    #[cfg(target_os = "linux")]
    descriptor: std::os::fd::OwnedFd,
}

impl ProcessExitObserver {
    pub(super) async fn wait(self, deadline: Instant) -> io::Result<()> {
        // These safe syscall wrappers block on the exact process event. Keep the
        // bounded wait off Tokio's executor, and join it before returning.
        tokio::task::spawn_blocking(move || self.wait_for_exit(deadline))
            .await
            .map_err(io::Error::other)?
    }
}

#[cfg(target_os = "macos")]
impl ProcessExitObserver {
    pub(super) fn observe(process_id: i32) -> io::Result<Self> {
        use nix::sys::event::{EventFilter, EventFlag, FilterFlag, KEvent, Kqueue};
        let queue = Kqueue::new()?;
        let process_id = usize::try_from(process_id).map_err(io::Error::other)?;
        let event = KEvent::new(
            process_id,
            EventFilter::EVFILT_PROC,
            EventFlag::EV_ADD | EventFlag::EV_ONESHOT,
            FilterFlag::NOTE_EXIT,
            0,
            0,
        );
        queue.kevent(&[event], &mut [], None)?;
        Ok(Self { queue })
    }

    fn wait_for_exit(&self, deadline: Instant) -> io::Result<()> {
        use nix::sys::event::{EventFilter, EventFlag, FilterFlag, KEvent};
        let event = KEvent::new(
            0,
            EventFilter::EVFILT_PROC,
            EventFlag::empty(),
            FilterFlag::empty(),
            0,
            0,
        );
        loop {
            let remaining = deadline.saturating_duration_since(std::time::Instant::now());
            let timeout = nix::libc::timespec {
                tv_sec: remaining.as_secs().try_into().map_err(io::Error::other)?,
                tv_nsec: remaining.subsec_nanos().into(),
            };
            let mut events = [event];
            match self.queue.kevent(&[], &mut events, Some(timeout)) {
                Ok(0) => return Err(io::ErrorKind::TimedOut.into()),
                Ok(_) if events[0].flags().contains(EventFlag::EV_ERROR) => {
                    return Err(io::Error::from_raw_os_error(
                        events[0].data().try_into().map_err(io::Error::other)?,
                    ));
                }
                Ok(_) => return Ok(()),
                Err(nix::errno::Errno::EINTR) => continue,
                Err(error) => return Err(error.into()),
            }
        }
    }
}

#[cfg(target_os = "linux")]
impl ProcessExitObserver {
    pub(super) fn observe(process_id: i32) -> io::Result<Self> {
        let process_id = rustix::process::Pid::from_raw(process_id)
            .ok_or_else(|| io::Error::other("operator process ID is invalid"))?;
        let descriptor =
            rustix::process::pidfd_open(process_id, rustix::process::PidfdFlags::empty())?;
        Ok(Self { descriptor })
    }

    fn wait_for_exit(&self, deadline: Instant) -> io::Result<()> {
        use rustix::event::{PollFd, PollFlags, Timespec, poll};
        loop {
            let remaining = deadline.saturating_duration_since(std::time::Instant::now());
            let timeout = Timespec {
                tv_sec: remaining.as_secs().try_into().map_err(io::Error::other)?,
                tv_nsec: remaining.subsec_nanos().into(),
            };
            let mut descriptors = [PollFd::new(&self.descriptor, PollFlags::IN)];
            match poll(&mut descriptors, Some(&timeout)) {
                Ok(0) => return Err(io::ErrorKind::TimedOut.into()),
                Ok(_) if descriptors[0].revents().contains(PollFlags::IN) => return Ok(()),
                Ok(_) => return Err(io::Error::other("Host process exit observation failed")),
                Err(rustix::io::Errno::INTR) => continue,
                Err(error) => return Err(error.into()),
            }
        }
    }
}
