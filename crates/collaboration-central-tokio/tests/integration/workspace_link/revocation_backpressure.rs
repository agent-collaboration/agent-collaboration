//! Force the actual Axum socket's write to return Pending without a peer read.
use super::fixture::{Fixture, receive, send};
use super::link_support::{caller, create_workspace};
use axum::serve::Listener;
use central_link_protocol::{DataFrame, LinkFrame, LinkRequest};
use collaboration_central_conformance::{ConformanceResult as Result, signin_fixtures as signed};
use collaboration_central_tokio::WorkspaceLink;
use serde_json::json;
use std::{
    io,
    net::{Ipv4Addr, SocketAddr},
    pin::Pin,
    sync::{
        Arc, Mutex, PoisonError,
        atomic::{AtomicBool, Ordering},
    },
    task::{Context, Poll, Waker},
};
use tokio::{
    io::{AsyncRead, AsyncWrite, ReadBuf},
    net::{TcpListener, TcpStream},
    sync::Notify,
};
use tokio_util::sync::CancellationToken;

#[tokio::test]
async fn signed_revocation_drops_the_transport_with_a_reply_blocked_in_io() -> Result<()> {
    let fixture = Fixture::new().await?;
    let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).await?;
    let address = listener.local_addr()?;
    let gate = Arc::new(WriteGate::default());
    let link = WorkspaceLink::new(fixture.host.clone());
    let router = link.router();
    let stop = CancellationToken::new();
    let listener_stop = stop.clone();
    let gated = GatedListener {
        listener,
        gate: gate.clone(),
    };
    let task = tokio::spawn(async move {
        axum::serve(gated, router)
            .with_graceful_shutdown(listener_stop.cancelled_owned())
            .await
    });
    let result = revoke_while_reply_is_blocked(&fixture, address, &gate).await;
    link.begin_shutdown();
    stop.cancel();
    task.await??;
    link.shutdown().await;
    fixture.close().await?;
    result
}

async fn revoke_while_reply_is_blocked(
    fixture: &Fixture,
    address: SocketAddr,
    gate: &WriteGate,
) -> Result<()> {
    use reqwest_websocket::RequestBuilderExt;

    let mut socket = fixture
        .client()?
        .get(format!(
            "http://{address}/v1/organizations/{}/link",
            signed::ORGANIZATION
        ))
        .bearer_auth(&fixture.token)
        .upgrade()
        .send()
        .await?
        .into_websocket()
        .await?;
    if !matches!(
        receive(&mut socket).await?,
        LinkFrame::Data(DataFrame::Welcome { .. })
    ) {
        return Err("machine did not receive Welcome".into());
    }
    let private = reqwest::Client::builder()
        .no_proxy()
        .unix_socket(fixture.signin_socket.clone())
        .build()?;
    let revocation = signed::RevocationJourney {
        client: &private,
        origin: "http://localhost",
        organization: signed::ORGANIZATION,
        now: signed::NOW_SECONDS,
    };
    let owner = revocation.prepare().await?;
    gate.blocked.store(true, Ordering::SeqCst);
    let request = LinkRequest::new(
        "019f0000-0000-7000-8000-00000000e300".parse()?,
        caller()?,
        create_workspace("019f0000-0000-7000-8000-00000000e301", "Blocked reply")?,
    )?;
    send(
        &mut socket,
        &LinkFrame::Data(DataFrame::Request {
            request: Box::new(request),
        }),
    )
    .await?;
    // No peer reads from here. Wait for Pending at the Axum socket's actual write boundary.
    gate.pending.notified().await;
    revocation
        .expect(
            &owner,
            fixture.enrollment_id.as_str(),
            json!({"result":"revoked"}),
        )
        .await?;
    // Challenge a stranded write, even if it registered only for IO readiness.
    gate.after_revoke.store(true, Ordering::SeqCst);
    if let Some(waker) = gate
        .waker
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .take()
    {
        waker.wake();
    }
    let result = tokio::select! {
        biased;
        () = gate.dropped.notified() => Ok(()),
        () = gate.repolled.notified() => Err("revocation left the request reply polling a blocked transport".into()),
    };
    drop(socket);
    result
}

#[derive(Default)]
struct WriteGate {
    blocked: AtomicBool,
    after_revoke: AtomicBool,
    pending: Notify,
    repolled: Notify,
    dropped: Notify,
    waker: Mutex<Option<Waker>>,
}

struct GatedListener {
    listener: TcpListener,
    gate: Arc<WriteGate>,
}

impl Listener for GatedListener {
    type Io = GatedSocket;
    type Addr = SocketAddr;

    async fn accept(&mut self) -> (Self::Io, Self::Addr) {
        let (stream, address) = Listener::accept(&mut self.listener).await;
        (
            GatedSocket {
                stream,
                gate: self.gate.clone(),
            },
            address,
        )
    }

    fn local_addr(&self) -> io::Result<Self::Addr> {
        self.listener.local_addr()
    }
}

struct GatedSocket {
    stream: TcpStream,
    gate: Arc<WriteGate>,
}

impl Drop for GatedSocket {
    fn drop(&mut self) {
        self.gate.dropped.notify_one();
    }
}

impl AsyncRead for GatedSocket {
    fn poll_read(
        mut self: Pin<&mut Self>,
        context: &mut Context<'_>,
        buffer: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        Pin::new(&mut self.stream).poll_read(context, buffer)
    }
}

impl AsyncWrite for GatedSocket {
    fn poll_write(
        mut self: Pin<&mut Self>,
        context: &mut Context<'_>,
        buffer: &[u8],
    ) -> Poll<io::Result<usize>> {
        if !self.gate.blocked.load(Ordering::SeqCst) {
            return Pin::new(&mut self.stream).poll_write(context, buffer);
        }
        *self
            .gate
            .waker
            .lock()
            .unwrap_or_else(PoisonError::into_inner) = Some(context.waker().clone());
        if self.gate.after_revoke.load(Ordering::SeqCst) {
            self.gate.repolled.notify_one();
        }
        self.gate.pending.notify_one();
        Poll::Pending
    }

    fn poll_flush(mut self: Pin<&mut Self>, context: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.stream).poll_flush(context)
    }

    fn poll_shutdown(mut self: Pin<&mut Self>, context: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.stream).poll_shutdown(context)
    }
}
