//! The existing private debug control advances the Host's one ManualClock.
use chrono::{DateTime, Utc};
use serde_json::{Value, json};
use std::{io, os::unix::fs::OpenOptionsExt, path::Path, time::Duration};
use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader},
    net::{UnixListener, UnixStream},
};

pub fn prepare(directory: &Path) -> io::Result<UnixListener> {
    use std::os::unix::fs::PermissionsExt;
    let socket = directory.join("clock-control.sock");
    let listener = UnixListener::bind(&socket)?;
    std::fs::set_permissions(&socket, std::fs::Permissions::from_mode(0o600))?;
    let input = json!({"now":"2026-11-04T00:00:00Z","controlSocket":socket});
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(directory.join("messaging-test-mail-clock.json"))?;
    std::io::Write::write_all(
        &mut file,
        &serde_json::to_vec(&input).map_err(io::Error::other)?,
    )?;
    Ok(listener)
}

pub struct HostClockControl {
    control: BufReader<UnixStream>,
    acknowledged_utc: DateTime<Utc>,
}

impl HostClockControl {
    pub async fn connected(listener: UnixListener) -> io::Result<Self> {
        let (stream, _) = listener.accept().await?;
        let mut control = BufReader::new(stream);
        let acknowledged_utc = read_reply(&mut control, "ready").await?;
        Ok(Self {
            control,
            acknowledged_utc,
        })
    }

    pub async fn advance(&mut self, delay: Duration) -> io::Result<()> {
        let delta = chrono::Duration::from_std(delay).map_err(io::Error::other)?;
        let requested = self
            .acknowledged_utc
            .checked_add_signed(delta)
            .ok_or_else(|| io::Error::other("clock control exceeds UTC range"))?;
        let mut frame = serde_json::to_vec(&json!({"now":requested})).map_err(io::Error::other)?;
        frame.push(b'\n');
        self.control.get_mut().write_all(&frame).await?;
        let acknowledged = read_reply(&mut self.control, "stepped").await?;
        if acknowledged != requested {
            return Err(io::Error::other("Host acknowledged a different clock step"));
        }
        self.acknowledged_utc = acknowledged;
        Ok(())
    }
}

async fn read_reply(
    control: &mut BufReader<UnixStream>,
    expected: &str,
) -> io::Result<DateTime<Utc>> {
    let mut frame = String::new();
    if control.read_line(&mut frame).await? == 0 {
        return Err(io::Error::other("Host closed its clock control"));
    }
    let reply: Value = serde_json::from_str(&frame).map_err(io::Error::other)?;
    if reply.get("kind").and_then(Value::as_str) != Some(expected) {
        return Err(io::Error::other("unexpected Host clock reply"));
    }
    serde_json::from_value(
        reply
            .get("now")
            .cloned()
            .ok_or_else(|| io::Error::other("Host clock reply omitted UTC"))?,
    )
    .map_err(io::Error::other)
}
