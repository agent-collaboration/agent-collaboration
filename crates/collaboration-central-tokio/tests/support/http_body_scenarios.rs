//! Real HTTP/1.1 sends on one physical socket exercise Worker refusal handling.
use collaboration_central_conformance::{ConformanceResult, access_client_id, owner_assertion};
use collaboration_central_core::{
    BODY_ORDER_HEADER, BodyObservationId, BodyOrderCommand, BodyOrderObservation,
};
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

enum BodyOrderEvidence {
    NotObserved,
    AfterBody,
    BeforeBody,
}

async fn observe_body(
    client: &reqwest::Client,
    host: &str,
    command: BodyOrderCommand,
) -> ConformanceResult<BodyOrderObservation> {
    Ok(client
        .post(format!("http://{host}/__conformance/admission"))
        .header("Cf-Access-Jwt-Assertion", owner_assertion()?)
        .json(&json!({"kind":"bodyOrder","value":{"command":command}}))
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?)
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
    ) -> ConformanceResult<(Reply, BodyOrderEvidence)> {
        let auth = assertion.map_or_else(String::new, |value| {
            format!("Cf-Access-Jwt-Assertion: {value}\r\n")
        });
        let observation: Option<BodyObservationId> = if withhold_suffix {
            Some(
                "019f0000-0000-7000-8000-000000000540"
                    .to_owned()
                    .try_into()?,
            )
        } else {
            None
        };
        let annotation = observation.as_ref().map_or_else(String::new, |id| {
            format!("{BODY_ORDER_HEADER}: {}\r\n", id.as_str())
        });
        let head = format!(
            "{method} {path} HTTP/1.1\r\nHost: {}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: keep-alive\r\n{auth}{annotation}\r\n",
            self.host,
            body.len()
        );
        let mut waiting_for = "owner capture registration";
        tokio::time::timeout(Duration::from_secs(10), async {
            let client = reqwest::Client::builder().no_proxy().build()?;
            if let Some(id) = observation.as_ref()
                && observe_body(&client, &self.host, BodyOrderCommand::Begin { request: id.clone() }).await?
                    != (BodyOrderObservation::Reading { bytes_read: 0 })
            {
                return Err("body-order capture was not registered before the upload".into());
            }
            waiting_for = "request header write";
            self.socket.get_mut().write_all(head.as_bytes()).await?;
            let mut status_prefix = Vec::new();
            let early_response = if let Some(id) = observation.as_ref() {
                let (prefix, suffix) = body.split_at(1);
                self.socket.get_mut().write_all(prefix).await?;
                self.socket.get_mut().flush().await?;
                // Keep partially read response bytes across cancellation. The
                // actual adapter read releases the suffix; a response wins only
                // when its real head arrives, never by an absence interval.
                waiting_for = "actual body prefix read or premature response head";
                let early = tokio::select! {
                    result = self.socket.read_until(b'\n', &mut status_prefix) => {
                        if result? == 0 { return Err("HTTP connection ended before its response head".into()); }
                        true
                    }
                    observed = observe_body(&client, &self.host, BodyOrderCommand::AwaitRead {
                        request: id.clone(),
                        minimum_bytes: std::num::NonZeroU64::new(1).ok_or("positive body prefix")?,
                    }) => {
                        match observed? {
                            BodyOrderObservation::Reading { bytes_read: 1 } => {
                                println!("HTTP_BODY_EVENT actual_prefix_read bytes=1 suffix_held=true");
                                false
                            }
                            BodyOrderObservation::ResponseBeforeEnd { .. } => true,
                            other => return Err(format!("body prefix was not consumed by the actual stream: {other:?}").into()),
                        }
                    }
                };
                waiting_for = "withheld body suffix write";
                self.socket.get_mut().write_all(suffix).await?;
                early
            } else {
                waiting_for = "complete body write";
                self.socket.get_mut().write_all(body).await?;
                false
            };
            waiting_for = "body flush and response following body EOF";
            self.socket.get_mut().flush().await?;
            let reply = self.reply(status_prefix).await?;
            let order = if let Some(id) = observation {
                waiting_for = "actual body EOF and response-order observation";
                let completed = observe_body(&client, &self.host, BodyOrderCommand::Finish { request: id }).await?;
                match completed {
                    BodyOrderObservation::ResponseBeforeEnd { .. } => BodyOrderEvidence::BeforeBody,
                    _ if early_response => BodyOrderEvidence::BeforeBody,
                    BodyOrderObservation::ResponseAfterEnd { bytes_read }
                        if bytes_read == u64::try_from(body.len())? => {
                            println!("HTTP_BODY_EVENT actual_response_after_eof bytes={bytes_read}");
                            BodyOrderEvidence::AfterBody
                        }
                    other => return Err(format!("body EOF/response ordering was not observed: {other:?}").into()),
                }
            } else {
                BodyOrderEvidence::NotObserved
            };
            Ok::<_, Box<dyn std::error::Error + Send + Sync>>((reply, order))
        })
        .await
        .map_err(|error| {
            format!("HTTP body request exceeded its existing bound while waiting for {waiting_for}: {error}")
        })?
        .map_err(|error| {
            format!("HTTP body request failed while waiting for {waiting_for}: {error}").into()
        })
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
    async fn reply(&mut self, mut status_prefix: Vec<u8>) -> ConformanceResult<Reply> {
        if !status_prefix.ends_with(b"\n")
            && self.socket.read_until(b'\n', &mut status_prefix).await? == 0
        {
            return Err("HTTP connection closed before its response status".into());
        }
        if status_prefix.len() > 8192 {
            return Err("HTTP fixture response header exceeded its bound".into());
        }
        let status = String::from_utf8(status_prefix)?
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
    match early_response {
        BodyOrderEvidence::BeforeBody => {
            return Err("production refusal arrived before its request body completed".into());
        }
        BodyOrderEvidence::NotObserved => {
            return Err("Worker body-order observation was missing".into());
        }
        BodyOrderEvidence::AfterBody => {}
    }
    println!("HTTP_BODY_SCOPE body order: Worker adapter only; native counterpart not exercised");
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
