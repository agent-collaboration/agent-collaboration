//! A C8 stand-in checks the client's real form requests, timing and rotating family.
//! It does not prove Better Auth or central admission; SO8 owns that interaction.
use std::{
    collections::HashMap,
    sync::{
        Arc, Mutex,
        atomic::{AtomicI64, Ordering},
    },
};

use axum::{
    Json, Router,
    extract::{Form, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::post,
};
use serde_json::{Value, json};
use tokio_util::sync::CancellationToken;

use super::super::{
    device_code_flow::run_device_code_login,
    login_contract::RefreshCredentialKeeping,
};
use super::*;

const MACHINE_CLIENT: &str = "019f0000-0000-7000-8000-000000000010";
const DEVICE_CODE: &str = "f11-device-code-canary";

struct ControlledClock(AtomicI64);
impl FleetLoginClock for ControlledClock {
    fn now(&self) -> SyncTime {
        SyncTime::from_unix_millis(self.0.load(Ordering::SeqCst))
    }
    fn wait_until(&self, deadline: SyncTime) -> LoginFuture<'_, Result<(), ()>> {
        Box::pin(async move {
            self.0.store(deadline.unix_millis(), Ordering::SeqCst);
            Ok(())
        })
    }
}

enum GrantState {
    Unrequested,
    Polling {
        last_poll_ms: i64,
        interval_ms: i64,
        polls: u8,
    },
    Issued {
        refresh: String,
        generation: u64,
    },
}
struct StandIn {
    clock: Arc<ControlledClock>,
    verification_uri: String,
    grant: Mutex<GrantState>,
    requests: Mutex<Vec<HashMap<String, String>>>,
    poll_times: Mutex<Vec<i64>>,
    redemption_reply: RedemptionReply,
}

#[derive(Clone, Copy)]
enum RedemptionReply {
    Complete,
    LoseAfterConsume,
    MalformedAfterConsume,
}

fn response(status: StatusCode, body: Value) -> Response {
    (status, Json(body)).into_response()
}
fn error(code: &str) -> Response {
    response(StatusCode::BAD_REQUEST, json!({"error":code}))
}

async fn authorize(
    State(state): State<Arc<StandIn>>,
    Form(form): Form<HashMap<String, String>>,
) -> Response {
    state.requests.lock().unwrap().push(form.clone());
    let expected = [
        ("client_id", "fixture-device-client"),
        ("scope", "offline_access"),
        ("resource", "https://central.example.invalid/machine"),
        (
            "machine_installation_id",
            "019f0000-0000-7000-8000-000000000002",
        ),
        ("machine_peer_id", "019f0000-0000-7000-8000-000000000003"),
        ("machine_label", "Fixture machine"),
        ("organization_id", "019f0000-0000-7000-8000-000000000001"),
    ];
    if form.len() != expected.len()
        || expected
            .iter()
            .any(|(key, value)| form.get(*key).map(String::as_str) != Some(*value))
    {
        return error("invalid_request");
    }
    *state.grant.lock().unwrap() = GrantState::Polling {
        last_poll_ms: 0,
        interval_ms: 5_000,
        polls: 0,
    };
    response(
        StatusCode::OK,
        json!({
            "device_code":DEVICE_CODE, "user_code":"ABCD-EFGH", "verification_uri":state.verification_uri,
            "verification_uri_complete":format!("{}?user_code=ABCD-EFGH", state.verification_uri), "expires_in":600, "interval":5,
        }),
    )
}

fn token_body(state: &StandIn, refresh: &str, generation: u64) -> Value {
    let now = state.clock.now().unix_millis() / 1000;
    let header = URL_SAFE_NO_PAD.encode(br#"{"alg":"RS256","kid":"fixture"}"#);
    let payload = URL_SAFE_NO_PAD
        .encode(serde_json::to_vec(&json!({"exp":now+3600,"grant_expires_at":now+600})).unwrap());
    // An obviously fake signature; the Router only reads timing here. F10/SO8
    // verify the real issuer's signature at central.
    json!({"access_token":format!("{header}.{payload}.f11-access-canary-{generation}"), "refresh_token":refresh,
        "token_type":"Bearer", "expires_in":3600, "machine_client_id":MACHINE_CLIENT, "scope":"offline_access"})
}

async fn token(
    State(state): State<Arc<StandIn>>,
    Form(form): Form<HashMap<String, String>>,
) -> Response {
    state.requests.lock().unwrap().push(form.clone());
    if form.get("resource").map(String::as_str) != Some("https://central.example.invalid/machine") {
        return error("invalid_request");
    }
    match form.get("grant_type").map(String::as_str) {
        Some("urn:ietf:params:oauth:grant-type:device_code") => {
            if form.get("client_id").map(String::as_str) != Some("fixture-device-client")
                || form.get("device_code").map(String::as_str) != Some(DEVICE_CODE)
            {
                return error("invalid_client");
            }
            let now = state.clock.now().unix_millis();
            state.poll_times.lock().unwrap().push(now);
            let mut grant = state.grant.lock().unwrap();
            let GrantState::Polling {
                last_poll_ms,
                interval_ms,
                polls,
            } = &mut *grant
            else {
                return error("invalid_grant");
            };
            if now < *last_poll_ms + *interval_ms {
                *interval_ms += 5_000;
                return error("slow_down");
            }
            *last_poll_ms = now;
            *polls += 1;
            if *polls == 1 {
                return error("authorization_pending");
            }
            if *polls == 2 {
                *interval_ms += 5_000;
                return error("slow_down");
            }
            let refresh = "f11-refresh-canary-0".to_owned();
            let body = token_body(&state, &refresh, 0);
            *grant = GrantState::Issued {
                refresh,
                generation: 0,
            };
            match state.redemption_reply {
                RedemptionReply::Complete => response(StatusCode::OK, body),
                RedemptionReply::MalformedAfterConsume => response(
                    StatusCode::OK,
                    json!({"access_token":"malformed-success-secret-canary"}),
                ),
                RedemptionReply::LoseAfterConsume => {
                    // The grant has already been consumed above. The body
                    // stream fails by an event, with no timeout or real clock.
                    let stream = futures_util::stream::once(async {
                        Err::<String, _>(std::io::Error::other("injected lost reply after consume"))
                    });
                    Response::new(axum::body::Body::from_stream(stream))
                }
            }
        }
        Some("refresh_token") => {
            if form.get("client_id").map(String::as_str) != Some(MACHINE_CLIENT) {
                return error("invalid_client");
            }
            let mut grant = state.grant.lock().unwrap();
            let GrantState::Issued {
                refresh,
                generation,
            } = &mut *grant
            else {
                return error("invalid_grant");
            };
            if form.get("refresh_token").map(String::as_str) != Some(refresh.as_str()) {
                return error("invalid_grant");
            }
            *generation += 1;
            *refresh = format!("f11-refresh-canary-{generation}");
            response(StatusCode::OK, token_body(&state, refresh, *generation))
        }
        _ => error("unsupported_grant_type"),
    }
}

#[tokio::test]
async fn fleet_machine_login_http_flow_keeps_machine_fields_client_rotation_and_code_display_real()
{
    let listener = tokio::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0))
        .await
        .unwrap();
    let address = listener.local_addr().unwrap();
    let clock = Arc::new(ControlledClock(AtomicI64::new(0)));
    let state = Arc::new(StandIn {
        clock: clock.clone(),
        verification_uri: format!("http://{address}/activate"),
        grant: Mutex::new(GrantState::Unrequested),
        requests: Mutex::new(Vec::new()),
        poll_times: Mutex::new(Vec::new()),
        redemption_reply: RedemptionReply::Complete,
    });
    let router = Router::new()
        .route("/device", post(authorize))
        .route("/token", post(token))
        .with_state(state.clone());
    let stop = CancellationToken::new();
    let shutdown = stop.clone();
    let server = tokio::spawn(async move {
        axum::serve(listener, router)
            .with_graceful_shutdown(shutdown.cancelled_owned())
            .await
    });
    let configuration: super::super::login_files::FleetLoginConfiguration = serde_json::from_value(json!({
        "deviceAuthorizationUrl":format!("http://{address}/device"),"tokenUrl":format!("http://{address}/token"),
        "deviceClientId":"fixture-device-client","machineResource":"https://central.example.invalid/machine",
        "endpoint":"wss://central.example.invalid/v1/session","receiver":"019f0000-0000-7000-8000-000000000011",
        "relay":"019f0000-0000-7000-8000-000000000012","access":{"kind":"none"},
    })).unwrap();
    let issuer = HttpMachineTokenIssuer::new(
        configuration.device_authorization_url,
        configuration.token_url,
        configuration.device_client_id,
        configuration.machine_resource,
        clock.clone(),
    )
    .unwrap();
    let request = DeviceLoginRequest {
        organization_id: "019f0000-0000-7000-8000-000000000001"
            .to_owned()
            .try_into()
            .unwrap(),
        installation: "019f0000-0000-7000-8000-000000000002"
            .to_owned()
            .try_into()
            .unwrap(),
        peer: serde_json::from_value(json!("019f0000-0000-7000-8000-000000000003")).unwrap(),
        label: "Fixture machine".to_owned().try_into().unwrap(),
    };
    let mut display = None;
    let issued = run_device_code_login(
        &issuer,
        clock.as_ref(),
        &request,
        RefreshCredentialKeeping::MemoryOnly,
        |event| display = Some(event),
    )
    .await
    .unwrap();
    assert_eq!(
        *state.poll_times.lock().unwrap(),
        vec![5_000, 10_000, 20_000]
    );
    assert_eq!(issued.credential.machine_client_id.as_str(), MACHINE_CLIENT);
    assert_eq!(issued.valid_until, SyncTime::from_unix_millis(620_000));
    let display = serde_json::to_string(&display.unwrap()).unwrap();
    assert!(display.contains("ABCD-EFGH"));
    assert!(!display.contains(DEVICE_CODE));
    assert!(!display.contains(issued.token.expose_secret()));
    assert!(!display.contains(issued.credential.refresh_credential.expose_secret()));
    assert!(!display.contains("verification_uri_complete"));
    assert!(!display.contains("user_code="));
    let first = issued.credential.clone();
    let rotated = issuer.refresh(&first).await.unwrap();
    assert_eq!(
        rotated.credential.refresh_credential.expose_secret(),
        "f11-refresh-canary-1"
    );
    let twice = issuer.refresh(&rotated.credential).await.unwrap();
    assert_eq!(
        twice.credential.refresh_credential.expose_secret(),
        "f11-refresh-canary-2"
    );
    assert!(matches!(
        issuer.refresh(&first).await,
        Err(IssuerFailure::InvalidGrant)
    ));
    let mut wrong = twice.credential;
    wrong.machine_client_id = "019f0000-0000-7000-8000-000000000099"
        .to_owned()
        .try_into()
        .unwrap();
    assert!(issuer.refresh(&wrong).await.is_err());
    stop.cancel();
    server.await.unwrap().unwrap();
}

#[tokio::test]
async fn fleet_machine_login_http_consume_then_lost_or_malformed_success_is_unknown_without_replay()
{
    use super::super::{device_code_flow::DeviceCodeFailure, login_contract::FleetLoginUnknown};
    for redemption_reply in [
        RedemptionReply::LoseAfterConsume,
        RedemptionReply::MalformedAfterConsume,
    ] {
        let listener = tokio::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0))
            .await
            .unwrap();
        let address = listener.local_addr().unwrap();
        let clock = Arc::new(ControlledClock(AtomicI64::new(0)));
        let state = Arc::new(StandIn {
            clock: clock.clone(),
            verification_uri: format!("http://{address}/activate"),
            grant: Mutex::new(GrantState::Unrequested),
            requests: Mutex::new(Vec::new()),
            poll_times: Mutex::new(Vec::new()),
            redemption_reply,
        });
        let router = Router::new()
            .route("/device", post(authorize))
            .route("/token", post(token))
            .with_state(state.clone());
        let stop = CancellationToken::new();
        let shutdown = stop.clone();
        let server = tokio::spawn(async move {
            axum::serve(listener, router)
                .with_graceful_shutdown(shutdown.cancelled_owned())
                .await
        });
        let issuer = HttpMachineTokenIssuer::new(
            format!("http://{address}/device").try_into().unwrap(),
            format!("http://{address}/token").try_into().unwrap(),
            "fixture-device-client".to_owned().try_into().unwrap(),
            "https://central.example.invalid/machine"
                .to_owned()
                .try_into()
                .unwrap(),
            clock.clone(),
        )
        .unwrap();
        let request = DeviceLoginRequest {
            organization_id: "019f0000-0000-7000-8000-000000000001"
                .to_owned()
                .try_into()
                .unwrap(),
            installation: "019f0000-0000-7000-8000-000000000002"
                .to_owned()
                .try_into()
                .unwrap(),
            peer: serde_json::from_value(json!("019f0000-0000-7000-8000-000000000003")).unwrap(),
            label: "Fixture machine".to_owned().try_into().unwrap(),
        };

        let result = run_device_code_login(
            &issuer,
            clock.as_ref(),
            &request,
            RefreshCredentialKeeping::MemoryOnly,
            |_| {},
        )
        .await;

        assert!(
            matches!(result, Err(DeviceCodeFailure::OutcomeUnknown(FleetLoginUnknown::DeviceRedemption { organization_id, installation })) if organization_id == request.organization_id && installation == request.installation)
        );
        assert!(
            matches!(*state.grant.lock().unwrap(), GrantState::Issued { .. }),
            "the server consumed before losing its response"
        );
        assert_eq!(
            *state.poll_times.lock().unwrap(),
            vec![5_000, 10_000, 20_000],
            "no automatic replay after the consumed code"
        );
        stop.cancel();
        server.await.unwrap().unwrap();
    }
}
