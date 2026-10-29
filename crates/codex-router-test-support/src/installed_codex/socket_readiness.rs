//! Kernel socket readiness with the caller's original absolute failure deadline.
use rustix::event::{PollFd, PollFlags, Timespec, poll};
use std::{io, net::TcpListener, time::Instant};

pub(super) fn wait_for_socket_readiness(
    descriptors: &mut [PollFd<'_>],
    deadline: Option<Instant>,
) -> io::Result<()> {
    loop {
        let remaining = deadline
            .map(|deadline| Timespec::try_from(deadline.saturating_duration_since(Instant::now())))
            .transpose()
            .map_err(|error| io::Error::new(io::ErrorKind::InvalidInput, error))?;
        match poll(descriptors, remaining.as_ref()) {
            Err(rustix::io::Errno::INTR) => continue,
            Err(error) => return Err(error.into()),
            Ok(0) => {
                return Err(io::Error::new(
                    io::ErrorKind::TimedOut,
                    "socket readiness deadline elapsed",
                ));
            }
            Ok(_) => {
                if descriptors.iter().any(|descriptor| {
                    descriptor
                        .revents()
                        .intersects(PollFlags::ERR | PollFlags::NVAL)
                }) {
                    return Err(io::Error::other(
                        "socket readiness reported an invalid or failed descriptor",
                    ));
                }
                return Ok(());
            }
        }
    }
}

pub(super) fn wait_for_listener(listener: &TcpListener, deadline: Instant) -> io::Result<()> {
    let mut descriptors = [PollFd::new(listener, PollFlags::IN)];
    wait_for_socket_readiness(&mut descriptors, Some(deadline))?;
    if !descriptors
        .first()
        .is_some_and(|descriptor| descriptor.revents().contains(PollFlags::IN))
    {
        return Err(io::Error::other(
            "listener became unavailable before an incoming connection",
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::wait_for_listener;
    use std::{
        io::ErrorKind,
        io::{Read as _, Write as _},
        net::{TcpListener, TcpStream},
        time::{Duration, Instant},
    };

    #[test]
    fn real_http_request_crosses_the_upstream_accept_boundary() -> crate::FixtureResult {
        use super::super::{InstalledCodexSmokeMode, MockWebSocketUpstream};
        let upstream = MockWebSocketUpstream::start(InstalledCodexSmokeMode::HttpSse)?;
        let mut client = TcpStream::connect(upstream.address())?;
        client.set_read_timeout(Some(Duration::from_secs(2)))?;
        let body = r#"{"model":"fixture-model","stream":true}"#;
        write!(
            client,
            "POST /v1/responses HTTP/1.1\r\nhost: fixture\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
            body.len()
        )?;
        let mut response = String::new();
        client.read_to_string(&mut response)?;
        let transcript = upstream.join()?;
        let request = transcript
            .http_sse
            .ok_or("upstream omitted the real HTTP request")?;
        if !response.starts_with("HTTP/1.1 200 OK\r\n")
            || !response.contains("codex-router smoke ok")
            || request.request_line != "POST /v1/responses HTTP/1.1"
            || request.body != body
            || transcript.http_probe_count != 0
        {
            return Err("real upstream accept lost its exact request or response".into());
        }
        Ok(())
    }

    #[test]
    fn listener_readiness_preserves_the_actual_incoming_peer() -> crate::FixtureResult {
        let listener = TcpListener::bind("127.0.0.1:0")?;
        listener.set_nonblocking(true)?;
        let client = TcpStream::connect(listener.local_addr()?)?;
        wait_for_listener(&listener, Instant::now() + Duration::from_secs(5))?;
        let (_accepted, peer) = listener.accept()?;
        if peer != client.local_addr()? {
            return Err("readiness did not preserve the actual queued connection".into());
        }
        Ok(())
    }

    #[test]
    fn expired_listener_guard_cannot_report_a_connection() -> crate::FixtureResult {
        let listener = TcpListener::bind("127.0.0.1:0")?;
        listener.set_nonblocking(true)?;
        match wait_for_listener(&listener, Instant::now()) {
            Err(error) if error.kind() == ErrorKind::TimedOut => Ok(()),
            _ => Err("expired empty-listener guard reported readiness".into()),
        }
    }
}
