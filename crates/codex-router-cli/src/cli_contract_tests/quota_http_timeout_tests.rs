//! Real async HTTP timeout at the quota-provider boundary, driven by Tokio time.
use super::*;
use std::future::{Future, poll_fn};
use std::task::Poll;
use tokio::io::AsyncReadExt;

#[tokio::test(start_paused = true)]
async fn quota_refresh_http_provider_times_out_hanging_usage_endpoint() {
    // An active blocking task prevents automatic virtual-clock jumps during real
    // socket setup. Only the explicit advances below can expire this request.
    let (release_clock, clock_release) = mpsc::channel();
    let clock_guard =
        tokio::task::spawn_blocking(move || clock_release.recv_timeout(Duration::from_secs(5)));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let (request_received, observe_request) = tokio::sync::oneshot::channel();
    let server = tokio::spawn(async move {
        let (mut stream, _) = listener.accept().await.unwrap();
        let mut request = Vec::new();
        let mut buffer = [0; 1024];
        while !request.windows(4).any(|bytes| bytes == b"\r\n\r\n") {
            let read = stream.read(&mut buffer).await.unwrap();
            assert!(read > 0, "quota request must include complete headers");
            request.extend_from_slice(&buffer[..read]);
        }
        assert!(request.starts_with(b"GET /api/codex/usage HTTP/1.1\r\n"));
        request_received.send(()).unwrap();
        let mut remainder = Vec::new();
        stream.read_to_end(&mut remainder).await.unwrap();
        assert!(remainder.is_empty());
    });
    let provider = must_ok(HttpQuotaRefreshProvider::new_with_timeout(
        Duration::from_millis(10),
    ));
    let fetch = provider.fetch_quota(QuotaRefreshProviderRequest::new(
        account_id("acct_timeout"),
        "timeout",
        "responses",
        format!("http://{address}"),
        SecretString::new("timeout-token-canary"),
        None,
    ));
    tokio::pin!(fetch);
    tokio::select! {
        result = &mut fetch => panic!("quota fetch finished before its received-request deadline: {result:?}"),
        receipt = observe_request => receipt.expect("real server must receive complete request"),
    }
    tokio::time::advance(Duration::from_millis(9)).await;
    let before_deadline = poll_fn(|context| Poll::Ready(fetch.as_mut().poll(context))).await;
    assert!(
        before_deadline.is_pending(),
        "request must remain active before its 10ms deadline"
    );
    tokio::time::advance(Duration::from_millis(2)).await;
    let error = tokio::time::timeout(Duration::from_secs(2), &mut fetch)
        .await
        .expect("advanced request deadline must settle the fetch")
        .expect_err("hanging usage endpoint must time out");
    assert!(error.to_string().contains("quota refresh request failed"));
    assert!(!error.to_string().contains("timeout-token-canary"));
    tokio::time::timeout(Duration::from_secs(2), server)
        .await
        .unwrap()
        .unwrap();
    release_clock.send(()).unwrap();
    clock_guard
        .await
        .unwrap()
        .expect("real I/O must settle inside the failure guard");
}
