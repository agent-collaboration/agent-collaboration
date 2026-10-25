#![allow(clippy::unwrap_used)]

use axum::{
    Router,
    extract::ws::{Message, WebSocketUpgrade},
    http::HeaderMap,
    routing::get,
};
use board_fleet_sync::{AccessServiceToken, ReqwestSessionTransport};
use fleet_sync_protocol::{AuthRefusal, CloseReason};
use fleet_sync_session::{
    Bytes, ConnectFailure, ConnectionClosed, Endpoint, PeerCredential, SessionTransport,
    TransportConnection,
};
use std::{
    sync::{Arc, Mutex},
    time::Duration,
};
use tokio::sync::oneshot;

fn transport() -> ReqwestSessionTransport {
    ReqwestSessionTransport::new(
        AccessServiceToken::new("fixture-id", "fixture-secret").unwrap(),
        Duration::from_secs(5),
        Duration::from_secs(5),
    )
    .unwrap()
}

#[test]
fn access_credentials_reject_empty_and_invalid_headers_without_disclosure() {
    for (client_id, client_secret) in [
        ("", "fixture-secret"),
        ("fixture-id", ""),
        ("fixture-id\r\ninjected: value", "fixture-secret"),
        ("fixture-id", "fixture-secret\nvalue"),
    ] {
        assert!(matches!(
            AccessServiceToken::new(client_id, client_secret),
            Err(ConnectFailure::InvalidCredential)
        ));
    }
    let access = AccessServiceToken::new("fixture-id", "fixture-secret").unwrap();
    assert_eq!(format!("{access:?}"), "AccessServiceToken([redacted])");
}

#[tokio::test]
async fn http_credential_refusals_are_typed_and_redirects_are_not_followed() {
    use axum::http::StatusCode;
    use std::sync::atomic::{AtomicU64, Ordering};

    let redirects = Arc::new(AtomicU64::new(0));
    let redirected = redirects.clone();
    let app = Router::new()
        .route("/unauthorized", get(|| async { StatusCode::UNAUTHORIZED }))
        .route("/forbidden", get(|| async { StatusCode::FORBIDDEN }))
        .route(
            "/redirect",
            get(|| async { axum::response::Redirect::temporary("/destination") }),
        )
        .route(
            "/destination",
            get(move || async move {
                redirected.fetch_add(1, Ordering::SeqCst);
                StatusCode::UNAUTHORIZED
            }),
        );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let (stop, stopped) = oneshot::channel();
    let server = tokio::spawn(async move {
        axum::serve(listener, app)
            .with_graceful_shutdown(async {
                let _ = stopped.await;
            })
            .await
            .unwrap();
    });
    let credential = PeerCredential::new("Bearer fixture-machine").unwrap();
    let transport = transport();
    let unauthorized = transport
        .connect(
            &Endpoint::new(&format!("http://{address}/unauthorized")).unwrap(),
            &credential,
        )
        .await;
    let forbidden = transport
        .connect(
            &Endpoint::new(&format!("http://{address}/forbidden")).unwrap(),
            &credential,
        )
        .await;
    let redirect = transport
        .connect(
            &Endpoint::new(&format!("http://{address}/redirect")).unwrap(),
            &credential,
        )
        .await;
    let invalid_header = transport
        .connect(
            &Endpoint::new(&format!("http://{address}/unauthorized")).unwrap(),
            &PeerCredential::new("Bearer fixture\r\ninjected: value").unwrap(),
        )
        .await;
    let _ = stop.send(());
    tokio::time::timeout(Duration::from_secs(5), server)
        .await
        .unwrap()
        .unwrap();

    assert!(matches!(
        unauthorized,
        Err(ConnectFailure::CredentialRejected(AuthRefusal::Unknown))
    ));
    assert!(matches!(
        forbidden,
        Err(ConnectFailure::CredentialRejected(AuthRefusal::Unknown))
    ));
    assert!(matches!(redirect, Err(ConnectFailure::Unavailable)));
    assert_eq!(redirects.load(Ordering::SeqCst), 0);
    assert!(matches!(
        invalid_header,
        Err(ConnectFailure::InvalidCredential)
    ));
}

#[tokio::test]
async fn typed_refusal_header_requires_one_known_json_string_on_an_authentication_status() {
    use axum::http::{HeaderValue, StatusCode};
    let cases = [
        (vec![r#""unknown""#], AuthRefusal::Unknown),
        (vec![r#""expired""#], AuthRefusal::Expired),
        (vec![r#""revoked""#], AuthRefusal::Revoked),
        (vec![r#""wrongKind""#], AuthRefusal::WrongKind),
        (vec![], AuthRefusal::Unknown),
        (vec!["expired"], AuthRefusal::Unknown),
        (vec![r#"{"expired":null}"#], AuthRefusal::Unknown),
        (vec!["null"], AuthRefusal::Unknown),
        (vec!["1"], AuthRefusal::Unknown),
        (vec![r#""futureReason""#], AuthRefusal::Unknown),
        (vec![r#""expired", "revoked""#], AuthRefusal::Unknown),
        (vec![r#""expired""#, r#""expired""#], AuthRefusal::Unknown),
        (vec![r#""expired""#, r#""revoked""#], AuthRefusal::Unknown),
    ];
    let mut app = Router::new();
    for (index, (values, _)) in cases.iter().enumerate() {
        let mut headers = HeaderMap::new();
        for value in values {
            headers.append(
                "fleet-sync-auth-refusal",
                HeaderValue::from_str(value).unwrap(),
            );
        }
        for status in [StatusCode::UNAUTHORIZED, StatusCode::FORBIDDEN] {
            let headers = headers.clone();
            app = app.route(
                &format!("/case-{index}-{}", status.as_u16()),
                get(move || {
                    let headers = headers.clone();
                    async move { (status, headers) }
                }),
            );
        }
    }
    app = app.route(
        "/non-auth",
        get(|| async {
            (
                StatusCode::BAD_GATEWAY,
                [("fleet-sync-auth-refusal", r#""expired""#)],
            )
        }),
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let (stop, stopped) = oneshot::channel();
    let server = tokio::spawn(async move {
        axum::serve(listener, app)
            .with_graceful_shutdown(async {
                let _ = stopped.await;
            })
            .await
            .unwrap();
    });
    let transport = transport();
    let credential = PeerCredential::new("Bearer fixture-machine").unwrap();

    for (index, (_, expected)) in cases.into_iter().enumerate() {
        for status in [401, 403] {
            let endpoint =
                Endpoint::new(&format!("http://{address}/case-{index}-{status}")).unwrap();
            let refused = transport.connect(&endpoint, &credential).await;
            assert!(
                matches!(refused, Err(ConnectFailure::CredentialRejected(actual)) if actual == expected),
                "case {index}, status {status}"
            );
        }
    }
    assert!(matches!(
        transport
            .connect(
                &Endpoint::new(&format!("http://{address}/non-auth")).unwrap(),
                &credential
            )
            .await,
        Err(ConnectFailure::Unavailable)
    ));
    let _ = stop.send(());
    tokio::time::timeout(Duration::from_secs(5), server)
        .await
        .unwrap()
        .unwrap();
}

#[tokio::test]
async fn text_frames_close_the_binary_transport_with_a_typed_failure() {
    let app = Router::new().route(
        "/session",
        get(|upgrade: WebSocketUpgrade| async {
            upgrade.on_upgrade(|mut socket| async move {
                let _ = socket
                    .send(Message::Text("not a binary session frame".into()))
                    .await;
            })
        }),
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let (stop, stopped) = oneshot::channel();
    let server = tokio::spawn(async move {
        axum::serve(listener, app)
            .with_graceful_shutdown(async {
                let _ = stopped.await;
            })
            .await
            .unwrap();
    });
    let mut connection = transport()
        .connect(
            &Endpoint::new(&format!("ws://{address}/session")).unwrap(),
            &PeerCredential::new("Bearer fixture-machine").unwrap(),
        )
        .await
        .unwrap();
    let result = tokio::time::timeout(Duration::from_secs(5), connection.receive())
        .await
        .unwrap();
    let next = connection.receive().await;
    let send = connection.send(Bytes::from_static(b"after close")).await;
    connection.close(CloseReason::ClientClosed).await;
    let _ = stop.send(());
    tokio::time::timeout(Duration::from_secs(5), server)
        .await
        .unwrap()
        .unwrap();

    assert_eq!(result, Some(Err(ConnectionClosed::UnsupportedFrame)));
    assert_eq!(next, None);
    assert_eq!(send, Err(ConnectionClosed::Transport));
}

#[tokio::test]
async fn upgrade_carries_all_credentials_and_binary_frames() {
    let (observed, received) = oneshot::channel();
    let observed = Arc::new(Mutex::new(Some(observed)));
    let app = Router::new().route(
        "/session",
        get(move |headers: HeaderMap, upgrade: WebSocketUpgrade| {
            let observed = observed.clone();
            async move {
                let values = [
                    headers
                        .get("cf-access-client-id")
                        .is_some_and(|value| value == "fixture-id"),
                    headers
                        .get("cf-access-client-secret")
                        .is_some_and(|value| value == "fixture-secret"),
                    headers
                        .get("authorization")
                        .is_some_and(|value| value == "Bearer fixture-machine"),
                ];
                if let Some(observed) = observed.lock().unwrap().take() {
                    let _ = observed.send(values);
                }
                upgrade.on_upgrade(|mut socket| async move {
                    if let Some(Ok(Message::Binary(frame))) = socket.recv().await {
                        let _ = socket.send(Message::Binary(frame)).await;
                    }
                })
            }
        }),
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let (stop, stopped) = oneshot::channel();
    let server = tokio::spawn(async move {
        axum::serve(listener, app)
            .with_graceful_shutdown(async {
                let _ = stopped.await;
            })
            .await
            .unwrap();
    });
    let access = AccessServiceToken::new("fixture-id", "fixture-secret").unwrap();
    let transport =
        ReqwestSessionTransport::new(access, Duration::from_secs(5), Duration::from_secs(5))
            .unwrap();
    let mut connection = transport
        .connect(
            &Endpoint::new(&format!("ws://{address}/session")).unwrap(),
            &PeerCredential::new("Bearer fixture-machine").unwrap(),
        )
        .await
        .unwrap();
    let headers = tokio::time::timeout(Duration::from_secs(5), received)
        .await
        .unwrap()
        .unwrap();
    connection
        .send(Bytes::from_static(b"binary session frame"))
        .await
        .unwrap();
    let echoed = tokio::time::timeout(Duration::from_secs(5), connection.receive())
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    connection.close(CloseReason::ClientClosed).await;
    let _ = stop.send(());
    tokio::time::timeout(Duration::from_secs(5), server)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(headers, [true, true, true]);
    assert_eq!(echoed, Bytes::from_static(b"binary session frame"));
}
