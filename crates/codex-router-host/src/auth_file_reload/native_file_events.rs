//! Owned native filesystem subscription using the workspace's syscall adapters.
//!
//! Tokio's raw AsyncFd registration requires unsafe code in the locked release.
//! One owned blocking watcher keeps that outside our API: a private Unix-stream
//! close wakes the kernel wait, and Drop joins the thread before releasing it.
use std::{
    io,
    net::Shutdown,
    os::unix::net::UnixStream,
    path::{Path, PathBuf},
};
use tokio::sync::watch;

#[derive(Clone, Copy)]
enum FileEvent {
    Changed,
    Failed,
}

pub(super) struct AuthFileEvents {
    auth_path: PathBuf,
    events: watch::Receiver<FileEvent>,
    stop: UnixStream,
    worker: Option<std::thread::JoinHandle<()>>,
}

impl AuthFileEvents {
    pub(super) fn open(home: &Path) -> io::Result<Self> {
        let home = home.canonicalize()?;
        let auth_path = home.join("auth.json");
        let (stop, cancellation) = UnixStream::pair()?;
        let native = NativeFileWatch::open(&home, cancellation)?;
        let (sender, events) = watch::channel(FileEvent::Changed);
        let worker = std::thread::Builder::new()
            .name("codex-auth-file-watch".into())
            .spawn(move || {
                if native.observe_changes(&sender).is_err() {
                    sender.send_replace(FileEvent::Failed);
                }
            })?;
        Ok(Self {
            auth_path,
            events,
            stop,
            worker: Some(worker),
        })
    }

    pub(super) fn auth_path(&self) -> &Path {
        &self.auth_path
    }

    pub(super) async fn wait_for_change(&mut self) -> io::Result<()> {
        self.events
            .changed()
            .await
            .map_err(|_| io::Error::other("native watcher closed"))?;
        match *self.events.borrow_and_update() {
            FileEvent::Changed => Ok(()),
            FileEvent::Failed => Err(io::Error::other("native watcher failed")),
        }
    }
}

impl Drop for AuthFileEvents {
    fn drop(&mut self) {
        // EOF remains observable even if the thread has not entered its wait yet.
        let _woken = self.stop.shutdown(Shutdown::Both);
        if let Some(worker) = self.worker.take() {
            let _joined = worker.join();
        }
    }
}

#[cfg(target_os = "macos")]
struct NativeFileWatch {
    kernel: nix::sys::event::Kqueue,
    _directory: std::fs::File,
    auth_file: Option<std::fs::File>,
    auth_path: PathBuf,
    cancellation: UnixStream,
}

#[cfg(target_os = "macos")]
impl NativeFileWatch {
    fn open(home: &Path, cancellation: UnixStream) -> io::Result<Self> {
        use nix::sys::event::{EventFilter, EventFlag, FilterFlag, KEvent};
        use std::os::fd::AsRawFd;
        let kernel = nix::sys::event::Kqueue::new()?;
        rustix::io::fcntl_setfd(&kernel, rustix::io::FdFlags::CLOEXEC)?;
        let directory = std::fs::File::open(home)?;
        watch_vnode(&kernel, &directory)?;
        let cancel_event = KEvent::new(
            usize::try_from(cancellation.as_raw_fd()).map_err(io::Error::other)?,
            EventFilter::EVFILT_READ,
            EventFlag::EV_ADD | EventFlag::EV_CLEAR,
            FilterFlag::empty(),
            0,
            0,
        );
        kernel.kevent(
            &[cancel_event],
            &mut [],
            Some(nix::libc::timespec {
                tv_sec: 0,
                tv_nsec: 0,
            }),
        )?;
        let mut watch = Self {
            kernel,
            _directory: directory,
            auth_file: None,
            auth_path: home.join("auth.json"),
            cancellation,
        };
        watch.rearm_auth_inode()?;
        Ok(watch)
    }

    fn rearm_auth_inode(&mut self) -> io::Result<()> {
        use std::os::unix::fs::OpenOptionsExt;
        let file = match std::fs::OpenOptions::new()
            .read(true)
            .custom_flags(nix::libc::O_NONBLOCK)
            .open(&self.auth_path)
        {
            Ok(file) => Some(file),
            Err(error) if error.kind() == io::ErrorKind::NotFound => None,
            Err(error) => return Err(error),
        };
        if let Some(file) = &file {
            watch_vnode(&self.kernel, file)?;
        }
        // Install the new descriptor before closing the old one; a rename never
        // removes the permanent directory watch or reuses the old descriptor.
        self.auth_file = file;
        Ok(())
    }

    fn observe_changes(mut self, events: &watch::Sender<FileEvent>) -> io::Result<()> {
        use nix::sys::event::{EventFilter, EventFlag, FilterFlag, KEvent};
        use std::os::fd::AsRawFd;
        let cancel_id = usize::try_from(self.cancellation.as_raw_fd()).map_err(io::Error::other)?;
        let blank = KEvent::new(
            0,
            EventFilter::EVFILT_VNODE,
            EventFlag::empty(),
            FilterFlag::empty(),
            0,
            0,
        );
        let mut pending = [blank; 16];
        loop {
            let count = match self.kernel.kevent(&[], &mut pending, None) {
                Ok(count) => count,
                Err(nix::errno::Errno::EINTR) => continue,
                Err(error) => return Err(error.into()),
            };
            for event in pending.iter().take(count) {
                if event.ident() == cancel_id && event.filter()? == EventFilter::EVFILT_READ {
                    return Ok(());
                }
                if event.flags().contains(EventFlag::EV_ERROR) {
                    return Err(io::Error::from_raw_os_error(
                        event.data().try_into().map_err(io::Error::other)?,
                    ));
                }
            }
            // Directory events cover replacement; the current inode covers
            // subsequent in-place writes, which a directory vnode cannot see.
            self.rearm_auth_inode()?;
            events.send_replace(FileEvent::Changed);
        }
    }
}

#[cfg(target_os = "macos")]
fn watch_vnode(queue: &nix::sys::event::Kqueue, file: &std::fs::File) -> io::Result<()> {
    use nix::sys::event::{EventFilter, EventFlag, FilterFlag, KEvent};
    use std::os::fd::AsRawFd;
    let descriptor = usize::try_from(file.as_raw_fd()).map_err(io::Error::other)?;
    let event = KEvent::new(
        descriptor,
        EventFilter::EVFILT_VNODE,
        EventFlag::EV_ADD | EventFlag::EV_CLEAR,
        FilterFlag::NOTE_WRITE
            | FilterFlag::NOTE_EXTEND
            | FilterFlag::NOTE_ATTRIB
            | FilterFlag::NOTE_RENAME
            | FilterFlag::NOTE_DELETE
            | FilterFlag::NOTE_REVOKE,
        0,
        0,
    );
    queue.kevent(
        &[event],
        &mut [],
        Some(nix::libc::timespec {
            tv_sec: 0,
            tv_nsec: 0,
        }),
    )?;
    Ok(())
}

#[cfg(target_os = "linux")]
struct NativeFileWatch {
    descriptor: std::os::fd::OwnedFd,
    cancellation: UnixStream,
}

#[cfg(target_os = "linux")]
impl NativeFileWatch {
    fn open(home: &Path, cancellation: UnixStream) -> io::Result<Self> {
        use rustix::fs::inotify::{self, CreateFlags, WatchFlags};
        let descriptor = inotify::init(CreateFlags::CLOEXEC | CreateFlags::NONBLOCK)?;
        inotify::add_watch(
            &descriptor,
            home,
            WatchFlags::MODIFY
                | WatchFlags::CLOSE_WRITE
                | WatchFlags::ATTRIB
                | WatchFlags::CREATE
                | WatchFlags::DELETE
                | WatchFlags::MOVED_TO
                | WatchFlags::MOVED_FROM
                | WatchFlags::DELETE_SELF
                | WatchFlags::MOVE_SELF,
        )?;
        Ok(Self {
            descriptor,
            cancellation,
        })
    }

    fn observe_changes(self, events: &watch::Sender<FileEvent>) -> io::Result<()> {
        use rustix::event::{PollFd, PollFlags, poll};
        loop {
            let mut ready = [
                PollFd::new(&self.cancellation, PollFlags::IN),
                PollFd::new(&self.descriptor, PollFlags::IN),
            ];
            match poll(&mut ready, None) {
                Ok(_) => {}
                Err(rustix::io::Errno::INTR) => continue,
                Err(error) => return Err(error.into()),
            }
            let mut ready = ready.iter();
            if ready.next().is_some_and(|fd| !fd.revents().is_empty()) {
                return Ok(());
            }
            if ready.next().is_some_and(|fd| !fd.revents().is_empty())
                && self.auth_file_changed()?
            {
                events.send_replace(FileEvent::Changed);
            }
        }
    }

    fn auth_file_changed(&self) -> io::Result<bool> {
        use rustix::fs::inotify::{ReadFlags, Reader};
        let mut storage = [std::mem::MaybeUninit::uninit(); 4096];
        let mut reader = Reader::new(&self.descriptor, &mut storage);
        let mut changed = false;
        loop {
            match reader.next() {
                Ok(event) => {
                    if event.events().contains(ReadFlags::IGNORED) {
                        return Err(io::Error::other("auth directory watch removed"));
                    }
                    if event.events().contains(ReadFlags::QUEUE_OVERFLOW)
                        || event
                            .file_name()
                            .is_none_or(|name| name.to_bytes() == b"auth.json")
                    {
                        changed = true;
                    }
                    // Return between kernel batches so a busy directory cannot
                    // starve the cancellation descriptor during Host shutdown.
                    if reader.is_buffer_empty() {
                        return Ok(changed);
                    }
                }
                Err(rustix::io::Errno::INTR) => continue,
                Err(rustix::io::Errno::AGAIN) => return Ok(changed),
                Err(error) => return Err(error.into()),
            }
        }
    }
}
