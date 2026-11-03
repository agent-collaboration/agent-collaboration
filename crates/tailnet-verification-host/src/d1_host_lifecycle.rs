//! Protocol-only native readiness. Never starts a thread, turn, login or model request.
use std::{
    fs::{DirBuilder, OpenOptions},
    io::{BufRead, BufReader, Read, Write},
    net::TcpListener,
    os::unix::fs::{DirBuilderExt, OpenOptionsExt, PermissionsExt},
    path::Path,
    process::{ChildStdin, Command, Stdio},
    sync::mpsc::{Receiver, sync_channel},
    time::Duration,
};

use chrono::Utc;
use serde::Deserialize;
use serde_json::{Value, json};

use crate::{
    bounded_command::{require_continuing, wait_for_child},
    d1_host_plan::{D1NativeReadinessPlan, require_unaliased_ancestors},
    driver_error::{DriverError, DriverResult},
    driver_records::{save_record, sha256},
    owned_host::with_owned_child,
    process_identity::observe_owned,
};

fn private_directory(path: &Path) -> DriverResult<()> {
    DirBuilder::new().mode(0o700).create(path)?;
    Ok(())
}

fn write_private(path: &Path, bytes: &[u8]) -> DriverResult<()> {
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(path)?;
    file.write_all(bytes)?;
    Ok(())
}

fn native_config(port: u16) -> String {
    format!(
        "model = \"gpt-6-luna\"\n\
         model_provider = \"codex-router-debug\"\n\
         cli_auth_credentials_store = \"ephemeral\"\n\
         [features]\n\
         enable_request_compression = false\n\
         [model_providers.codex-router-debug]\n\
         name = \"OpenAI\"\n\
         base_url = \"http://127.0.0.1:{port}/v1\"\n\
         wire_api = \"responses\"\n\
         requires_openai_auth = false\n\
         supports_websockets = true\n"
    )
}

#[derive(Deserialize)]
struct NativeFrame {
    id: Option<String>,
    method: Option<String>,
    result: Option<Value>,
    error: Option<Value>,
}

fn parse_answer(bytes: &[u8], expected: &str) -> DriverResult<Option<Value>> {
    let frame: NativeFrame = serde_json::from_slice(bytes)?;
    if frame.id.is_none() && frame.method.is_some() {
        return Ok(None);
    }
    if frame.id.as_deref() != Some(expected) || frame.method.is_some() || frame.error.is_some() {
        return Err(DriverError::Runtime(
            "native readiness reply rejected or mismatched",
        ));
    }
    frame
        .result
        .map(Some)
        .ok_or(DriverError::Runtime("native readiness result absent"))
}

fn await_answer(
    input: &Receiver<DriverResult<Vec<u8>>>,
    expected: &str,
    plan: &D1NativeReadinessPlan,
) -> DriverResult<Value> {
    for _ in 0..32 {
        require_continuing()?;
        let remaining = (plan.deadline.with_timezone(&Utc) - Utc::now())
            .to_std()
            .map_err(|_| DriverError::TimedOut("D1 qualification deadline"))?;
        let bytes = input
            .recv_timeout(remaining.min(Duration::from_secs(30)))
            .map_err(|_| DriverError::TimedOut("native readiness reply"))??;
        if let Some(answer) = parse_answer(&bytes, expected)? {
            return Ok(answer);
        }
    }
    Err(DriverError::Runtime(
        "too many native readiness notifications",
    ))
}

fn send(writer: &mut ChildStdin, frame: &Value) -> DriverResult<()> {
    serde_json::to_writer(&mut *writer, frame)?;
    writer.write_all(b"\n")?;
    writer.flush()?;
    Ok(())
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct AccountReadiness {
    account: Value,
    requires_openai_auth: bool,
}

fn require_no_client_auth(value: Value) -> DriverResult<()> {
    let account: AccountReadiness = serde_json::from_value(value)?;
    if account.requires_openai_auth || !account.account.is_null() {
        return Err(DriverError::Runtime(
            "fresh native home unexpectedly requires or has account auth",
        ));
    }
    Ok(())
}

pub fn run_native_readiness(plan: &D1NativeReadinessPlan) -> DriverResult<()> {
    plan.validate()?;
    if sha256(&plan.native.path)? != plan.native.sha256 {
        return Err(DriverError::Invalid("D1 declared native hash mismatch"));
    }
    let allocation = plan.repository.join("tmp/d1-verify/allocation");
    if !allocation.exists() {
        private_directory(&allocation)?;
    }
    require_unaliased_ancestors(&allocation)?;
    private_directory(&plan.run_root)?;
    let home = plan.run_root.join("codex");
    private_directory(&home)?;
    let package = home.join("packages");
    private_directory(&package)?;
    let standalone = package.join("standalone");
    private_directory(&standalone)?;
    let current = standalone.join("current");
    private_directory(&current)?;
    let binary = current.join("codex");
    std::fs::copy(&plan.native.path, &binary)?;
    std::fs::set_permissions(&binary, std::fs::Permissions::from_mode(0o700))?;
    if sha256(&binary)? != plan.native.sha256 {
        return Err(DriverError::Invalid("staged D1 native bytes differ"));
    }
    let upstream_guard = TcpListener::bind(("127.0.0.1", 0))?;
    upstream_guard.set_nonblocking(true)?;
    let config = native_config(upstream_guard.local_addr()?.port());
    write_private(&home.join("config.toml"), config.as_bytes())?;
    save_record(&plan.run_root.join("qualification-plan.json"), plan)?;
    let stderr = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(plan.run_root.join("native.stderr"))?;
    let owner_home = std::env::var_os("HOME").ok_or(DriverError::Invalid("HOME unavailable"))?;
    let mut child = Command::new(std::env::current_exe()?)
        .arg("__launch-host")
        .arg(&binary)
        .args(["app-server", "--listen", "stdio://"])
        .env_clear()
        .env("HOME", owner_home)
        .env("PATH", "/usr/bin:/bin:/usr/sbin:/sbin")
        .env("CODEX_HOME", &home)
        .env("TMPDIR", &plan.run_root)
        .env("CODEX_INTERNAL_APP_SERVER_REMOTE_CONTROL_DISABLED", "1")
        .current_dir(&plan.run_root)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(stderr)
        .spawn()?;
    let result = with_owned_child(&mut child, &plan.run_root, |child, known| {
        let stdout = child
            .stdout
            .take()
            .ok_or(DriverError::Runtime("native stdout absent"))?;
        let (sender, receiver) = sync_channel(8);
        let reader = std::thread::spawn(move || {
            let mut reader = BufReader::new(stdout);
            loop {
                let mut bytes = Vec::new();
                let read = reader
                    .by_ref()
                    .take(1024 * 1024 + 1)
                    .read_until(b'\n', &mut bytes);
                let answer = match read {
                    Ok(0) => break,
                    Ok(_) if bytes.len() > 1024 * 1024 => {
                        Err(DriverError::Invalid("native reply exceeds bound"))
                    }
                    Ok(_) => Ok(bytes),
                    Err(error) => Err(error.into()),
                };
                if sender.send(answer).is_err() {
                    break;
                }
            }
        });
        let observed = (|| {
            let initialize_id = uuid::Uuid::now_v7().to_string();
            let account_id = uuid::Uuid::now_v7().to_string();
            let mut writer = child
                .stdin
                .take()
                .ok_or(DriverError::Runtime("native stdin absent"))?;
            send(
                &mut writer,
                &json!({"id":initialize_id,"method":"initialize","params":{
                    "clientInfo":{"name":"codex-router-d1-verification","title":"D1 verification","version":env!("CARGO_PKG_VERSION")},
                    "capabilities":{"experimentalApi":true}
                }}),
            )?;
            let initialized = await_answer(&receiver, &initialize_id, plan)?;
            if initialized.get("codexHome").and_then(Value::as_str) != home.to_str() {
                return Err(DriverError::Runtime(
                    "native initialized with a different Codex home",
                ));
            }
            send(&mut writer, &json!({"method":"initialized"}))?;
            send(
                &mut writer,
                &json!({"id":account_id,"method":"account/read","params":{"refreshToken":false}}),
            )?;
            require_no_client_auth(await_answer(&receiver, &account_id, plan)?)?;
            let pid = i32::try_from(child.id())
                .map_err(|_| DriverError::Runtime("native PID overflow"))?;
            observe_owned(pid, known)?;
            drop(writer);
            let exit = wait_for_child(child, Duration::from_secs(10))?
                .ok_or(DriverError::TimedOut("native EOF shutdown"))?;
            if !exit.success() {
                return Err(DriverError::Runtime("native EOF shutdown failed"));
            }
            match upstream_guard.accept() {
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {}
                _ => {
                    return Err(DriverError::Runtime(
                        "native readiness attempted upstream traffic",
                    ));
                }
            }
            save_record(
                &plan.run_root.join("native-ready.json"),
                &json!({
                    "initialize":initialized,"account":null,"requiresOpenaiAuth":false,
                    "transport":"stdio","modelInputSent":false,"upstreamConnections":0,
                    "actualNativeSHA256":sha256(&binary)?,"authPolicy":plan.auth_policy,
                    "actualDriverSHA256":sha256(&std::env::current_exe()?)?,
                    "requestIds":[initialize_id,account_id],"upstreamGuardThrough":"nativeExit",
                    "limits":["not a Unix endpoint observation","no Router or model credential proof"]
                }),
            )?;
            Ok(())
        })();
        drop(receiver);
        // The outer owner settles the process before any reader join on a failed run.
        if observed.is_ok() {
            reader
                .join()
                .map_err(|_| DriverError::Runtime("native reader failed"))?;
        }
        observed
    });
    if home.join("auth.json").exists() {
        return Err(DriverError::Runtime(
            "native readiness unexpectedly created auth storage",
        ));
    }
    result?;
    println!(
        "{}",
        json!({"kind":"d1NativeReady","evidence":plan.run_root,"modelInputSent":false})
    );
    Ok(())
}

#[cfg(test)]
#[path = "d1_host_lifecycle_test.rs"]
mod tests;
