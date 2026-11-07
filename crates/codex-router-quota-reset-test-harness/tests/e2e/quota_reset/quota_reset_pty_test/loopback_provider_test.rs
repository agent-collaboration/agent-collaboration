use codex_router_quota_reset_test_harness::loopback_provider::HeldLoopbackProvider;
use std::{
    io::{Read, Write},
    net::TcpStream,
    time::Duration,
};

#[test]
fn quota_refresh_response_plan_is_opt_in_and_retains_real_get_post_gates() {
    let routing = "routing-provider-contract";
    for (mut provider, status, complete) in [
        (HeldLoopbackProvider::bind().unwrap(), 200, false),
        (
            HeldLoopbackProvider::bind_failing_usage_for_account(routing, 1, 429).unwrap(),
            429,
            false,
        ),
        (
            HeldLoopbackProvider::bind_releasing_first_gets(1).unwrap(),
            200,
            false,
        ),
        (
            HeldLoopbackProvider::bind_quota_refresh(&[]).unwrap(),
            200,
            true,
        ),
        (
            HeldLoopbackProvider::bind_quota_refresh(&[routing.to_owned()]).unwrap(),
            429,
            true,
        ),
    ] {
        let mut usage = TcpStream::connect(provider.address()).unwrap();
        usage
            .set_read_timeout(Some(Duration::from_secs(2)))
            .unwrap();
        write!(usage, "GET /backend-api/wham/usage HTTP/1.1\r\nhost: localhost\r\nchatgpt-account-id: {routing}\r\nconnection: close\r\n\r\n").unwrap();
        let requested = provider
            .wait_for_request_count(1, Duration::from_secs(2))
            .unwrap();
        assert_eq!(requested[0].method, "GET");
        assert_eq!(requested[0].response_status, status);
        provider.release_get_responses().unwrap();
        let mut response = String::new();
        usage.read_to_string(&mut response).unwrap();
        assert!(response.starts_with(&format!("HTTP/1.1 {status}")));
        if status == 200 {
            assert_eq!(
                response.contains("code_review_rate_limit"),
                complete,
                "the original grid provider payload remains the default"
            );
        }
        let mut reset = TcpStream::connect(provider.address()).unwrap();
        reset
            .set_read_timeout(Some(Duration::from_secs(2)))
            .unwrap();
        write!(reset, "POST /backend-api/wham/usage/reset HTTP/1.1\r\nhost: localhost\r\nchatgpt-account-id: {routing}\r\ncontent-length: 0\r\nconnection: close\r\n\r\n").unwrap();
        let requested = provider
            .wait_for_request_count(2, Duration::from_secs(2))
            .unwrap();
        assert_eq!(requested[1].method, "POST");
        provider.release_post_response().unwrap();
        let mut response = String::new();
        reset.read_to_string(&mut response).unwrap();
        assert!(response.contains("\"windows_reset\":2"));
        assert_eq!(provider.finish().unwrap().len(), 2);
    }
}
