//! Observe a direct owned child once in the kernel, and reap it on every outcome.
use std::{
    io,
    process::{Child, ExitStatus},
    sync::mpsc,
    time::Instant,
};

#[derive(Debug)]
pub(crate) enum ChildExitObservation {
    Exited(ExitStatus),
    DeadlineExpired(ExitStatus),
}

pub(crate) fn wait_for_sync_child(
    child: &mut Child,
    deadline: Instant,
) -> io::Result<ChildExitObservation> {
    match observe_sync_child(child, deadline) {
        Ok(observation) => Ok(observation),
        Err(error) => {
            let terminated = child.kill();
            let reaped = child.wait();
            Err(io::Error::new(
                error.kind(),
                format!("{error}; termination={terminated:?}; wait={reaped:?}"),
            ))
        }
    }
}

fn observe_sync_child(child: &mut Child, deadline: Instant) -> io::Result<ChildExitObservation> {
    if let Some(status) = child.try_wait()? {
        return Ok(ChildExitObservation::Exited(status));
    }
    let identity = rustix::process::Pid::from_child(child);
    let (observed, received) = mpsc::sync_channel(1);
    std::thread::scope(|scope| {
        let waiting = std::thread::Builder::new()
            .name("owned-cli-child-exit".to_owned())
            .spawn_scoped(scope, move || {
                let result = loop {
                    match rustix::process::waitid(
                        rustix::process::WaitId::Pid(identity),
                        rustix::process::WaitIdOptions::EXITED
                            | rustix::process::WaitIdOptions::NOWAIT,
                    ) {
                        Err(rustix::io::Errno::INTR) => continue,
                        result => break result.map(|status| status.is_some()),
                    }
                };
                let _sent = observed.send(result);
            })?;
        let result = received.recv_timeout(deadline.saturating_duration_since(Instant::now()));
        let exited = matches!(result, Ok(Ok(true)));
        let terminated = if exited { Ok(()) } else { child.kill() };
        // NOWAIT retains this exact PID until the observer has settled, even
        // when it starts after the parent requested termination at the bound.
        let joined = waiting.join();
        let status = child.wait();
        terminated?;
        joined.map_err(|_| io::Error::other("owned child exit observer panicked"))?;
        let status = status?;
        match result {
            Ok(Ok(true)) => Ok(ChildExitObservation::Exited(status)),
            Err(mpsc::RecvTimeoutError::Timeout) => {
                Ok(ChildExitObservation::DeadlineExpired(status))
            }
            Ok(Err(error)) => Err(error.into()),
            Ok(Ok(false)) | Err(mpsc::RecvTimeoutError::Disconnected) => {
                Err(io::Error::other("owned child exit observation unavailable"))
            }
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        os::unix::process::ExitStatusExt as _,
        process::{Command, Stdio},
        time::Duration,
    };

    #[test]
    fn direct_child_exit_retains_status_and_reaps_before_return() -> crate::FixtureResult {
        let mut child = Command::new("/bin/sh").args(["-c", "exit 7"]).spawn()?;
        let observed = wait_for_sync_child(&mut child, Instant::now() + Duration::from_secs(5))?;
        let ChildExitObservation::Exited(status) = observed else {
            return Err("completed child was reported as a deadline expiry".into());
        };
        if status.code() != Some(7) || child.wait()? != status {
            return Err("owned child status or cached reap was not retained".into());
        }
        Ok(())
    }

    #[test]
    fn expired_deadline_kills_and_reaps_the_held_child_before_return() -> crate::FixtureResult {
        let mut child = Command::new("/bin/sh")
            .args(["-c", "read fixture_input"])
            .stdin(Stdio::piped())
            .spawn()?;
        let observed = wait_for_sync_child(&mut child, Instant::now())?;
        let ChildExitObservation::DeadlineExpired(status) = observed else {
            return Err("held child unexpectedly completed before its controlled expiry".into());
        };
        if status.signal() != Some(9) || child.wait()? != status {
            return Err("deadline return preceded actual termination and reap".into());
        }
        Ok(())
    }
}
