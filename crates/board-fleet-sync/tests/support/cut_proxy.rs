use std::{io, net::SocketAddr, time::Duration};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt, copy_bidirectional},
    net::{TcpListener, TcpStream},
    sync::{mpsc, oneshot, watch},
    task::JoinHandle,
    time::timeout,
};

pub struct CutProxy {
    pub address: SocketAddr,
    pub upgrades: mpsc::Receiver<[bool; 3]>,
    pub cut: oneshot::Sender<()>,
    pub stop: watch::Sender<bool>,
    pub task: JoinHandle<io::Result<()>>,
}

impl CutProxy {
    pub async fn start(upstream: SocketAddr) -> io::Result<Self> {
        let listener = TcpListener::bind("127.0.0.1:0").await?;
        let address = listener.local_addr()?;
        let (observed, upgrades) = mpsc::channel(4);
        let (cut, mut cutting) = oneshot::channel();
        let (stop, mut stopped) = watch::channel(false);
        let task = tokio::spawn(async move {
            let mut first = true;
            loop {
                let mut downstream = tokio::select! {
                    accepted = listener.accept() => accepted?.0,
                    _ = stopped.changed() => break,
                };
                let mut upstream = TcpStream::connect(upstream).await?;
                let header = timeout(Duration::from_secs(5), read_header(&mut downstream))
                    .await
                    .map_err(io::Error::other)??;
                let text = String::from_utf8_lossy(&header).to_ascii_lowercase();
                let values = [
                    text.lines()
                        .any(|line| line == "cf-access-client-id: fixture-id"),
                    text.lines()
                        .any(|line| line == "cf-access-client-secret: fixture-secret"),
                    text.lines()
                        .any(|line| line == "authorization: bearer fixture-machine"),
                ];
                upstream.write_all(&header).await?;
                let _ = observed.send(values).await;
                tokio::select! {
                    result = copy_bidirectional(&mut downstream, &mut upstream) => { result?; },
                    _ = &mut cutting, if first => {},
                    _ = stopped.changed() => break,
                }
                first = false;
                // Dropping both real TCP streams cuts the connection without a
                // WebSocket close frame. The next accept is a fresh connection.
            }
            Ok(())
        });
        Ok(Self {
            address,
            upgrades,
            cut,
            stop,
            task,
        })
    }
}

async fn read_header(socket: &mut TcpStream) -> io::Result<Vec<u8>> {
    let mut header = Vec::new();
    while !header.ends_with(b"\r\n\r\n") {
        if header.len() >= 16 * 1024 {
            return Err(io::Error::other("fixture upgrade header too large"));
        }
        header.push(socket.read_u8().await?);
    }
    Ok(header)
}
