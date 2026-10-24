//! Real HTTP/1.1 sends on one physical socket exercise Worker refusal handling.
use collaboration_central_conformance::{ConformanceResult, access_client_id, owner_assertion};
use serde_json::json;
use std::time::Duration;
use tokio::{
    io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader},
    net::TcpStream,
};

const BODY_LIMIT: usize = 2_097_152;
struct Connection {
    socket: BufReader<TcpStream>,
    host: String,
}
struct Reply {
    status: u16,
    body: Vec<u8>,
}
impl Connection {
    async fn open(base_url: &str) -> ConformanceResult<Self> {
        let url = reqwest::Url::parse(base_url)?;
        let host = format!(
            "{}:{}",
            url.host_str().ok_or("HTTP fixture host missing")?,
            url.port_or_known_default()
                .ok_or("HTTP fixture port missing")?
        );
        let socket = TcpStream::connect(&host).await?;
        Ok(Self {
            socket: BufReader::new(socket),
            host,
        })
    }
    async fn request(
        &mut self,
        method: &str,
        path: &str,
        assertion: Option<&str>,
        body: &[u8],
        withhold_suffix: bool,
    ) -> ConformanceResult<(Reply, bool)> {
        let auth = assertion.map_or_else(String::new, |value| {
            format!("Cf-Access-Jwt-Assertion: {value}\r\n")
        });
        let head = format!(
            "{method} {path} HTTP/1.1\r\nHost: {}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: keep-alive\r\n{auth}\r\n",
            self.host,
            body.len()
        );
        tokio::time::timeout(Duration::from_secs(10), async {
            self.socket.get_mut().write_all(head.as_bytes()).await?;
            let early_response = if withhold_suffix {
                let (prefix, suffix) = body.split_at(1);
                self.socket.get_mut().write_all(prefix).await?;
                self.socket.get_mut().flush().await?;
                // The socket becoming readable is the early-response barrier.
                // A bounded observation window releases a correct server that
                // waits for the withheld upload; it is not server readiness.
                let early = tokio::time::timeout(
                    Duration::from_millis(250),
                    self.socket.get_mut().readable(),
                )
                .await
                .is_ok();
                self.socket.get_mut().write_all(suffix).await?;
                early
            } else {
                self.socket.get_mut().write_all(body).await?;
                false
            };
            self.socket.get_mut().flush().await?;
            Ok((self.reply().await?, early_response))
        })
        .await?
    }
    async fn line(&mut self) -> ConformanceResult<String> {
        let mut line = String::new();
        if self.socket.read_line(&mut line).await? == 0 {
            return Err("HTTP connection closed before its next response".into());
        }
        if line.len() > 8192 {
            return Err("HTTP fixture response header exceeded its bound".into());
        }
        Ok(line)
    }
    async fn reply(&mut self) -> ConformanceResult<Reply> {
        let status = self
            .line()
            .await?
            .split_whitespace()
            .nth(1)
            .ok_or("HTTP response status missing")?
            .parse()?;
        let mut headers = Vec::new();
        loop {
            let line = self.line().await?;
            if line == "\r\n" {
                break;
            }
            let (name, value) = line.split_once(':').ok_or("invalid HTTP response header")?;
            headers.push((name.to_ascii_lowercase(), value.trim().to_owned()));
            if headers.len() > 64 {
                return Err("too many HTTP fixture headers".into());
            }
        }
        let mut body = Vec::new();
        if let Some((_, length)) = headers.iter().find(|(name, _)| name == "content-length") {
            let length: usize = length.parse()?;
            if length > BODY_LIMIT {
                return Err("HTTP fixture response exceeded its bound".into());
            }
            body.resize(length, 0);
            self.socket.read_exact(&mut body).await?;
        } else if headers.iter().any(|(name, value)| {
            name == "transfer-encoding" && value.eq_ignore_ascii_case("chunked")
        }) {
            loop {
                let line = self.line().await?;
                let length = usize::from_str_radix(
                    line.trim()
                        .split(';')
                        .next()
                        .ok_or("HTTP chunk size missing")?,
                    16,
                )?;
                if length == 0 {
                    while self.line().await? != "\r\n" {}
                    break;
                }
                let through = body
                    .len()
                    .checked_add(length)
                    .ok_or("HTTP chunk overflow")?;
                if through > BODY_LIMIT {
                    return Err("HTTP chunked response exceeded its bound".into());
                }
                let before = body.len();
                body.resize(through, 0);
                self.socket
                    .read_exact(
                        body.get_mut(before..through)
                            .ok_or("HTTP chunk range missing")?,
                    )
                    .await?;
                if self.line().await? != "\r\n" {
                    return Err("HTTP chunk terminator missing".into());
                }
            }
        } else {
            return Err("HTTP fixture response has no bounded framing".into());
        }
        Ok(Reply { status, body })
    }
}
fn enrollment(installation: &str, client: &str) -> ConformanceResult<Vec<u8>> {
    Ok(serde_json::to_vec(
        &json!({"kind":"enrollMachine","value":{
            "installation":installation,"peer":"019f0000-0000-7000-8000-000000000541",
            "label":"HTTP body ownership fixture","access_client_id":access_client_id(client)?,
            "credential_hash":"a".repeat(64),"expires_at":"2033-05-18T03:33:20Z"
        }}),
    )?)
}
pub(super) async fn refused_then_valid_enrollment_uses_the_same_physical_connection(
    base_url: &str,
) -> ConformanceResult<()> {
    let mut connection = Connection::open(base_url).await?;
    let mut refused = enrollment("550e8400-e29b-41d4-a716-446655440541", "http-body-refused")?;
    refused.resize(BODY_LIMIT, b' ');
    let (first, early_response) = connection
        .request("POST", "/admin/v1/machines/enroll", None, &refused, true)
        .await?;
    if first.status != 401 {
        return Err(format!(
            "production missing assertion returned {} instead of401",
            first.status
        )
        .into());
    }
    // The owner uses the same proposed peer under another installation. Success
    // also proves that the refused request never reserved that peer identity.
    let valid = enrollment("550e8400-e29b-41d4-a716-446655440542", "http-body-valid")?;
    let (second, _) = connection
        .request(
            "POST",
            "/admin/v1/machines/enroll",
            Some(&owner_assertion()?),
            &valid,
            false,
        )
        .await?;
    if second.status != 200 {
        return Err(format!("production owner enrollment returned {}", second.status).into());
    }
    let decision: serde_json::Value = serde_json::from_slice(&second.body)?;
    if decision.get("kind") != Some(&json!("machineEnrolled"))
        || decision.pointer("/value/peer") != Some(&json!("019f0000-0000-7000-8000-000000000541"))
    {
        return Err(
            "refused enrollment wrote state or the same-connection owner send did not enroll"
                .into(),
        );
    }
    if early_response {
        return Err("production refusal arrived before its request body completed".into());
    }
    println!("HTTP_BODY_CASE refused_then_valid_enrollment_one_physical_connection passed");
    Ok(())
}
pub(super) async fn an_over_limit_body_is_refused_and_the_connection_is_closed(
    base_url: &str,
) -> ConformanceResult<()> {
    let owner = owner_assertion()?;
    let body = vec![b' '; BODY_LIMIT + 1];
    for (method, path, assertion) in [
        ("POST", "/admin/v1/machines/enroll", None),
        ("POST", "/admin/v1/machines/enroll", Some(owner.as_str())),
        ("GET", "/admin/v1/machines/enroll", Some(owner.as_str())),
        ("GET", "/v1/session", None),
        ("POST", "/unknown-route", Some(owner.as_str())),
        ("POST", "/admin/v1/conformance/bootstrap", None),
        ("POST", "/admin/v1/conformance/runtime", None),
        ("POST", "/__conformance/admission", None),
    ] {
        let mut connection = Connection::open(base_url).await?;
        let (reply, _) = connection
            .request(method, path, assertion, &body, false)
            .await?;
        if reply.status != 413 {
            return Err(format!(
                "over-limit {method} {path} returned {} instead of413",
                reply.status
            )
            .into());
        }
        let mut byte = [0_u8; 1];
        let end = tokio::time::timeout(Duration::from_secs(5), connection.socket.read(&mut byte))
            .await
            .map_err(|_| "over-limit response left the physical socket open")?;
        if !matches!(end, Ok(0) | Err(_)) {
            return Err("over-limit connection remained reusable".into());
        }
        println!("HTTP_BODY over-limit {method} {path}:413 and physical close");
    }
    println!("HTTP_BODY_CASE over_limit_413_and_physical_close passed");
    Ok(())
}
