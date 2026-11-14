//! Real workerd HTTP lifecycle, with the close/next-request ordering forced by events.
use super::*;
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
    sync::oneshot,
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum CloseOrdering {
    DeliverBeforeDispatch,
    WithholdUntilDispatch,
}
#[derive(Debug, Eq, PartialEq)]
enum ConnectionOutcome {
    FreshConnection,
    ReusedAfterUpstreamEof,
}

async fn relay_idle_close(
    listener: TcpListener,
    upstream_address: String,
    ordering: CloseOrdering,
    mut warm_response_consumed: oneshot::Receiver<()>,
    upstream_closed: oneshot::Sender<()>,
    finished: oneshot::Receiver<()>,
) -> ConnectionOutcome {
    let (mut client_stream, _) = listener.accept().await.unwrap();
    let mut upstream_stream = TcpStream::connect(&upstream_address).await.unwrap();
    let mut client_bytes = [0u8; 8192];
    let mut upstream_bytes = [0u8; 8192];
    let mut client_closed = false;
    // Relay the complete first real clock response, then wait for workerd's
    // actual EOF. No test clock, timer or sleep decides this transport event.
    loop {
        tokio::select! {
            biased;
            receipt = &mut warm_response_consumed => { receipt.unwrap(); break; },
            read = client_stream.read(&mut client_bytes), if !client_closed => {
                let read = read.unwrap();
                if read == 0 { client_closed = true; }
                else { upstream_stream.write_all(&client_bytes[..read]).await.unwrap(); }
            }
            read = upstream_stream.read(&mut upstream_bytes) => {
                let read = read.unwrap();
                assert_ne!(read, 0, "upstream closed before the first clock response");
                client_stream.write_all(&upstream_bytes[..read]).await.unwrap();
            }
        }
    }
    loop {
        tokio::select! {
            read = upstream_stream.read(&mut upstream_bytes) => {
                let read = read.unwrap();
                if read == 0 { break; }
                if !client_closed {
                    client_stream.write_all(&upstream_bytes[..read]).await.unwrap();
                }
            }
            read = client_stream.read(&mut client_bytes), if !client_closed => {
                let read = read.unwrap();
                assert_eq!(read, 0, "the next request started before the EOF barrier");
                // Deliberately retain the upstream until it supplies its own EOF.
                client_closed = true;
            }
        }
    }
    if ordering == CloseOrdering::DeliverBeforeDispatch && !client_closed {
        client_stream.shutdown().await.unwrap();
        assert_eq!(client_stream.read(&mut client_bytes).await.unwrap(), 0);
        client_closed = true;
    }
    eprintln!("real workerd EOF ordering={ordering:?}, client_closed={client_closed}");
    upstream_closed.send(()).unwrap();
    tokio::select! {
        read = client_stream.read(&mut client_bytes), if !client_closed => {
            assert_ne!(read.unwrap(), 0, "the old client socket closed unexpectedly");
            // The next POST has started on a socket whose real upstream has
            // closed. Release the withheld close without forwarding that POST.
            drop(client_stream);
            ConnectionOutcome::ReusedAfterUpstreamEof
        }
        accepted = listener.accept() => {
            let (mut fresh_client, _) = accepted.unwrap();
            let mut fresh_upstream = TcpStream::connect(&upstream_address).await.unwrap();
            tokio::select! {
                _ = tokio::io::copy_bidirectional(&mut fresh_client, &mut fresh_upstream) => {},
                _ = finished => {},
            }
            ConnectionOutcome::FreshConnection
        }
    }
}

#[tokio::test]
async fn control_http_connection_reopens_after_real_upstream_idle_close() {
    for ordering in [
        CloseOrdering::DeliverBeforeDispatch,
        CloseOrdering::WithholdUntilDispatch,
    ] {
        let clock = Arc::new(
            router_clock::ManualClock::starting_at(runtime_primitives::SyncTime::from_unix_millis(
                1_900_000_000_000,
            ))
            .unwrap(),
        );
        let host = WorkerdHost::open(clock.clone()).await.unwrap();
        let upstream = reqwest::Url::parse(&host.transport.origin).unwrap();
        let upstream_address = format!("127.0.0.1:{}", upstream.port().unwrap());
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let request_url = format!(
            "http://{}/test-support/workspace/019f0000-0000-7000-8000-00000000d201/clock",
            listener.local_addr().unwrap(),
        );
        let (warm_done, warm_receipt) = oneshot::channel();
        let (closed, close_receipt) = oneshot::channel();
        let (finish, finished) = oneshot::channel();
        let relay = tokio::spawn(relay_idle_close(
            listener,
            upstream_address,
            ordering,
            warm_receipt,
            closed,
            finished,
        ));
        let client = workerd_control_http_client();
        let warmed = client
            .post(&request_url)
            .json(&clock.now().unix_millis())
            .send()
            .await
            .unwrap()
            .json::<bool>()
            .await
            .unwrap();
        assert!(warmed);
        warm_done.send(()).unwrap();
        close_receipt.await.unwrap();
        // Exactly one subsequent call, never retried. The positive ordering
        // exposes EOF first; the race ordering exposes it after dispatch starts.
        let second = client
            .post(&request_url)
            .json(&clock.now().unix_millis())
            .send()
            .await;
        let accepted = match second {
            Ok(response) => response.json::<bool>().await.unwrap(),
            Err(failure) => {
                eprintln!(
                    "clock close ordering={ordering:?}: send_error request={} connect={} timeout={}",
                    failure.is_request(),
                    failure.is_connect(),
                    failure.is_timeout()
                );
                false
            }
        };
        let _ = finish.send(());
        let connection = relay.await.unwrap();
        host.close().await;
        assert_eq!(
            connection,
            ConnectionOutcome::FreshConnection,
            "{ordering:?}"
        );
        assert!(
            accepted,
            "the first attempt after upstream idle close must succeed"
        );
    }
}
