//! Observe upstream contact until the owned producer has actually stopped.
use super::{join_result, socket_readiness::wait_for_socket_readiness};
use rustix::event::{PollFd, PollFlags};
use std::{
    io::ErrorKind,
    net::{Shutdown, TcpListener},
    os::unix::net::UnixStream,
    thread,
};

struct ContactObservation {
    completed: UnixStream,
    task: thread::JoinHandle<Result<usize, String>>,
}

pub(super) struct MockNoConnectionUpstream {
    address: String,
    observation: Option<ContactObservation>,
}

impl MockNoConnectionUpstream {
    pub(super) fn start() -> Result<Self, String> {
        let listener = TcpListener::bind("127.0.0.1:0")
            .map_err(|error| format!("failed to bind no-connection upstream: {error}"))?;
        listener
            .set_nonblocking(true)
            .map_err(|error| format!("failed to configure no-connection upstream: {error}"))?;
        let address = listener
            .local_addr()
            .map_err(|error| format!("failed to read no-connection upstream address: {error}"))?
            .to_string();
        let (completed, completion) = UnixStream::pair()
            .map_err(|error| format!("failed to create upstream completion signal: {error}"))?;
        let task = thread::Builder::new()
            .name("codex-router-hostile-no-token-upstream".to_owned())
            .spawn(move || observe_contact(listener, completion))
            .map_err(|error| format!("failed to spawn no-connection upstream: {error}"))?;
        Ok(Self {
            address,
            observation: Some(ContactObservation { completed, task }),
        })
    }
    pub(super) fn address(&self) -> &str {
        &self.address
    }

    /// Called after the Router child has been stopped and reaped.
    pub(super) fn join(mut self) -> Result<usize, String> {
        let observation = self
            .observation
            .take()
            .ok_or("no-connection upstream was already joined")?;
        drop(observation.completed);
        join_result(observation.task, "no-connection upstream")
    }
}

impl Drop for MockNoConnectionUpstream {
    fn drop(&mut self) {
        if let Some(observation) = self.observation.take() {
            drop(observation.completed);
            let _joined = join_result(observation.task, "no-connection upstream cleanup");
        }
    }
}

fn observe_contact(listener: TcpListener, completed: UnixStream) -> Result<usize, String> {
    let mut descriptors = [
        PollFd::new(&listener, PollFlags::IN),
        PollFd::new(&completed, PollFlags::IN),
    ];
    wait_for_socket_readiness(&mut descriptors, None)
        .map_err(|error| format!("no-connection upstream observation failed: {error}"))?;
    // Contact wins when both facts are ready. After completion, inspect the
    // retained kernel queue once as well; the producer can create no later call.
    match listener.accept() {
        Ok((stream, _peer)) => {
            let _closed = stream.shutdown(Shutdown::Both);
            Ok(1)
        }
        Err(error) if error.kind() == ErrorKind::ConnectionAborted => Ok(1),
        Err(error) if error.kind() == ErrorKind::WouldBlock => {
            let complete = descriptors.get(1).is_some_and(|descriptor| {
                descriptor
                    .revents()
                    .intersects(PollFlags::IN | PollFlags::HUP)
            });
            if complete {
                Ok(0)
            } else {
                Err("upstream contact readiness had no queued connection or completion".to_owned())
            }
        }
        Err(error) => Err(format!("no-connection upstream accept failed: {error}")),
    }
}

#[cfg(test)]
mod tests {
    use super::MockNoConnectionUpstream;

    #[test]
    fn completed_producer_without_contact_reports_zero() -> crate::FixtureResult {
        let upstream = MockNoConnectionUpstream::start()?;
        if upstream.join()? != 0 {
            return Err("completed producer contacted the upstream".into());
        }
        Ok(())
    }

    #[test]
    fn real_contact_is_retained_when_completion_is_also_ready() -> crate::FixtureResult {
        let upstream = MockNoConnectionUpstream::start()?;
        let contact = std::net::TcpStream::connect(upstream.address())?;
        let observed = upstream.join()?;
        drop(contact);
        if observed != 1 {
            return Err("completion hid the real upstream connection".into());
        }
        Ok(())
    }
}
