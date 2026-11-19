use reqwest::StatusCode;
use serde_json::json;
use std::{os::unix::fs::PermissionsExt, process::Stdio};
use tokio::io::{AsyncBufReadExt, BufReader};

#[tokio::test]
async fn central_serve_publishes_only_the_link_and_drains_its_private_listener() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().join("central");
    let config = directory.path().join("central.jsonc");
    let socket = root.join("signin.sock");
    std::fs::write(
        &config,
        serde_json::to_vec(&json!({
            "listen":"127.0.0.1:0",
            "root":root,
            "signIn":{
                "issuer":"https://signin.example.invalid",
                "audience":"central-stamp",
                "adminAudience":"central-admin",
                "machineAudience":"http://127.0.0.1:49100",
                "publicOrigin":"https://central.example.invalid",
                "jwksUrl":"https://signin.example.invalid/jwks",
                "serviceId":"019f0000-0000-7000-8000-000000000001"
            }
        }))
        .unwrap(),
    )
    .unwrap();
    let mut server = tokio::process::Command::new(env!("CARGO_BIN_EXE_codex-router"))
        .args(["central", "serve", "--config"])
        .arg(&config)
        .stdout(Stdio::piped())
        .kill_on_drop(true)
        .spawn()
        .unwrap();
    let mut stdout = BufReader::new(server.stdout.take().unwrap()).lines();
    let ready = stdout.next_line().await.unwrap().unwrap();
    let (origin, announced_socket) = ready
        .strip_prefix("central listening: ")
        .and_then(|line| line.strip_suffix(')'))
        .and_then(|line| line.split_once(" (private sign-in socket "))
        .unwrap();
    assert_eq!(std::path::Path::new(announced_socket), socket);
    assert_eq!(
        std::fs::metadata(&root).unwrap().permissions().mode() & 0o777,
        0o700
    );
    assert_eq!(
        std::fs::metadata(&socket).unwrap().permissions().mode() & 0o777,
        0o600
    );

    let public = reqwest::Client::builder().no_proxy().build().unwrap();
    let private = reqwest::Client::builder()
        .unix_socket(socket.clone())
        .build()
        .unwrap();
    let private_route = "/internal/device-login-decisions";
    assert_eq!(
        public
            .get(format!("{origin}{private_route}"))
            .send()
            .await
            .unwrap()
            .status(),
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        private
            .get(format!("http://localhost{private_route}"))
            .send()
            .await
            .unwrap()
            .status(),
        StatusCode::METHOD_NOT_ALLOWED
    );
    assert_eq!(
        public
            .post(format!("{origin}/admin/v1/machines/enroll"))
            .send()
            .await
            .unwrap()
            .status(),
        StatusCode::NOT_FOUND
    );
    let link = public
        .get(format!(
            "{origin}/v1/organizations/019f0000-0000-7000-8000-000000000002/link"
        ))
        .header("Connection", "Upgrade")
        .header("Upgrade", "websocket")
        .header("Sec-WebSocket-Version", "13")
        .header("Sec-WebSocket-Key", "dGhlIHNhbXBsZSBub25jZQ==")
        .send()
        .await
        .unwrap();
    assert_eq!(link.status(), StatusCode::UNAUTHORIZED);

    let pid = i32::try_from(server.id().unwrap()).unwrap();
    nix::sys::signal::kill(
        nix::unistd::Pid::from_raw(pid),
        nix::sys::signal::Signal::SIGTERM,
    )
    .unwrap();
    assert_eq!(
        stdout.next_line().await.unwrap().as_deref(),
        Some("central stopped")
    );
    assert!(server.wait().await.unwrap().success());
    assert!(!socket.exists());
}
