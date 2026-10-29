//! Test clients begin after the real Serve command prints its completed bind receipt.
use super::*;
use std::net::SocketAddr;

pub(super) struct ServeReadinessOutput {
    bytes: Vec<u8>,
    bound: Option<mpsc::Sender<SocketAddr>>,
}

impl ServeReadinessOutput {
    pub(super) fn new(bound: mpsc::Sender<SocketAddr>) -> Self {
        Self {
            bytes: Vec::new(),
            bound: Some(bound),
        }
    }

    pub(super) fn into_bytes(self) -> Vec<u8> {
        self.bytes
    }
}

impl Write for ServeReadinessOutput {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        self.bytes.extend_from_slice(bytes);
        if self.bound.is_some() {
            for line in self.bytes.split_inclusive(|byte| *byte == b'\n') {
                if line.last() != Some(&b'\n') {
                    continue;
                }
                let Some(address) = line.strip_prefix(b"listening: ") else {
                    continue;
                };
                let address = std::str::from_utf8(address)
                    .map_err(std::io::Error::other)?
                    .trim()
                    .parse::<SocketAddr>()
                    .map_err(std::io::Error::other)?;
                if let Some(bound) = self.bound.take() {
                    bound.send(address).map_err(std::io::Error::other)?;
                }
                break;
            }
        }
        Ok(bytes.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

pub(super) fn await_serve_bind(observed: &mpsc::Receiver<SocketAddr>, expected_port: u16) {
    let bound = observed
        .recv_timeout(Duration::from_secs(5))
        .expect("real Serve must print its bound listener before the client connects");
    assert!(bound.ip().is_loopback());
    assert_eq!(bound.port(), expected_port);
}

#[test]
fn serve_bind_receipt_requires_a_complete_valid_line_and_preserves_output() {
    let (bound, observed) = mpsc::channel();
    let mut output = ServeReadinessOutput::new(bound);
    output
        .write_all(b"router ready\nlistening: 127.0.0.1:12")
        .unwrap();
    assert!(matches!(
        observed.try_recv(),
        Err(mpsc::TryRecvError::Empty)
    ));
    output.write_all(b"345\n").unwrap();
    assert_eq!(
        observed.try_recv().unwrap(),
        "127.0.0.1:12345".parse().unwrap()
    );
    output.write_all(b"listening: 127.0.0.1:23456\n").unwrap();
    assert!(matches!(
        observed.try_recv(),
        Err(mpsc::TryRecvError::Disconnected)
    ));
    assert_eq!(
        output.into_bytes(),
        b"router ready\nlistening: 127.0.0.1:12345\nlistening: 127.0.0.1:23456\n"
    );
}

#[test]
fn malformed_serve_bind_receipt_cannot_start_a_client() {
    let (bound, observed) = mpsc::channel();
    let mut output = ServeReadinessOutput::new(bound);
    assert!(output.write_all(b"listening: invalid-address\n").is_err());
    assert!(matches!(
        observed.try_recv(),
        Err(mpsc::TryRecvError::Empty)
    ));
}
