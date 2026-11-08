//! The owned workerd harness, the fixture caller Worker's client and a
//! person's DPoP-bound admin requests for the organization link journey.
use super::ADMIN_POLICY;
use super::organization_stamps::{
    self as stamps, ADMIN_AUDIENCE, ISSUER, NOW_SECONDS, PUBLIC_ORIGIN, ProofKey,
};
use collaboration_central_conformance::ConformanceResult;
use serde_json::{Value, json};
use std::{path::PathBuf, process::Stdio};
use tokio::{
    io::{AsyncBufReadExt, BufReader},
    process::{Child, Command},
};

pub(crate) struct LinkHarness {
    pub(crate) child: Child,
    pub(crate) output: tokio::task::JoinHandle<ConformanceResult<()>>,
    /// Every line the harness printed after readiness, Worker logs included.
    lines: std::sync::Arc<std::sync::Mutex<Vec<String>>>,
}
impl LinkHarness {
    pub(crate) async fn start(arguments: &[String]) -> ConformanceResult<Self> {
        let mut child = Command::new(arguments.first().ok_or("node path")?)
            .current_dir(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../.."))
            .arg("scripts/tooling/run-central-link-workerd.ts")
            .args(arguments.get(1..).ok_or("harness arguments")?)
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .kill_on_drop(true)
            .spawn()?;
        let mut lines = BufReader::new(child.stdout.take().ok_or("harness stdout")?).lines();
        // Readiness is the harness's own ready line; no polling or sleep.
        loop {
            let line = lines
                .next_line()
                .await?
                .ok_or("harness ended before readiness")?;
            println!("{line}");
            if line.starts_with("Ready on ")
                && line.ends_with("(real workerd, central-link binding)")
            {
                break;
            }
        }
        let kept = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let keeper = kept.clone();
        let output = tokio::spawn(async move {
            while let Some(line) = lines.next_line().await? {
                println!("{line}");
                keeper
                    .lock()
                    .map_err(|_| "harness lines poisoned")?
                    .push(line);
            }
            Ok(())
        });
        Ok(Self {
            child,
            output,
            lines: kept,
        })
    }
    /// Stop the harness and return every line it printed, all drained.
    pub(crate) async fn stop(mut self) -> ConformanceResult<Vec<String>> {
        let pid = self.child.id().ok_or("harness already exited")?;
        let signal = Command::new("kill")
            .args(["-INT", &pid.to_string()])
            .status()
            .await?;
        if !signal.success() {
            return Err("could not signal the owned harness".into());
        }
        let status = self.child.wait().await?;
        self.output.await??;
        if !matches!(status.code(), Some(0 | 130)) {
            return Err(format!("owned harness exited: {status}").into());
        }
        println!("LINK HARNESS reaped pid={pid} exit={:?}", status.code());
        let lines = self
            .lines
            .lock()
            .map_err(|_| "harness lines poisoned")?
            .clone();
        Ok(lines)
    }
}

#[derive(Clone)]
pub(crate) struct Caller {
    pub(crate) http: reqwest::Client,
    pub(crate) base: String,
    pub(crate) owner: String,
}
impl Caller {
    /// One request through the fixture caller: "/link" over the CENTRAL
    /// binding carries no Access assertion (the private entry must not need
    /// one); "/public" observer reads carry the owner's.
    pub(crate) async fn post(
        &self,
        path: &str,
        body: &Value,
    ) -> ConformanceResult<(u16, String, Value)> {
        self.send("POST", path, body).await
    }
    /// GET carries no body; any other method carries `body` as JSON.
    pub(crate) async fn send(
        &self,
        method: &str,
        path: &str,
        body: &Value,
    ) -> ConformanceResult<(u16, String, Value)> {
        let url = format!("{}{path}", self.base);
        let mut request = if method == "GET" {
            self.http.get(url)
        } else {
            self.http.post(url).json(body)
        };
        if path.starts_with("/public/") {
            request = request.header("Cf-Access-Jwt-Assertion", &self.owner);
        }
        let response = request.send().await?;
        let status = response.status().as_u16();
        let entry = response
            .headers()
            .get("x-collaboration-central-entry")
            .and_then(|value| value.to_str().ok())
            .unwrap_or("none")
            .to_owned();
        let bytes = response.bytes().await?;
        let body = if bytes.is_empty() {
            Value::Null
        } else {
            serde_json::from_slice(&bytes)
                .unwrap_or(Value::String(String::from_utf8_lossy(&bytes).into()))
        };
        Ok((status, entry, body))
    }
    pub(crate) async fn provision(
        &self,
        organization: &str,
        statement: &str,
    ) -> ConformanceResult<u16> {
        Ok(self
            .post(
                "/link/internal/organization-provisioning",
                &json!({"organizationId":organization,"organizationStatement":statement}),
            )
            .await?
            .0)
    }
    pub(crate) async fn digest(&self, organization: &str) -> ConformanceResult<(u16, Value)> {
        let (status, _, body) = self
            .post(
                "/link/internal/organization-statement-digest",
                &json!({"organizationId":organization}),
            )
            .await?;
        Ok((status, body))
    }
    /// A person's admin call over the link with a real token and DPoP proof.
    pub(crate) async fn admin(
        &self,
        path: &str,
        intent: &Value,
        body: &Value,
        person: &AdminPerson,
        jti_serial: u16,
    ) -> ConformanceResult<(u16, Value)> {
        self.admin_with(
            path,
            intent,
            body,
            &person.token()?,
            &person.key,
            jti_serial,
        )
        .await
    }
    /// One admin request with an already signed token.
    pub(crate) async fn admin_with(
        &self,
        path: &str,
        intent: &Value,
        body: &Value,
        token: &str,
        key: &ProofKey,
        jti_serial: u16,
    ) -> ConformanceResult<(u16, Value)> {
        let token = token.to_owned();
        let millis = NOW_SECONDS * 1000 - 1000;
        let jti = format!(
            "{:08x}-{:04x}-7000-8000-{:012x}",
            millis >> 16,
            millis & 0xffff,
            jti_serial
        );
        let proof = key.proof(&json!({
            "jti":jti,"htm":"POST","htu":format!("{PUBLIC_ORIGIN}{path}"),"iat":NOW_SECONDS - 1,
            "ath":stamps::sha256_url(token.as_bytes()),
            "intent":stamps::sha256_url(stamps::canonical(intent).as_bytes())
        }))?;
        let response = self
            .http
            .post(format!("{}/link{path}", self.base))
            .header("authorization", format!("DPoP {token}"))
            .header("dpop", proof)
            .json(body)
            .send()
            .await?;
        let status = response.status().as_u16();
        let bytes = response.bytes().await?;
        let body = if bytes.is_empty() {
            Value::Null
        } else {
            serde_json::from_slice(&bytes)?
        };
        Ok((status, body))
    }

    /// Read through public central's own Fleet routing, not the link.
    pub(crate) async fn records(&self, organization: &str) -> ConformanceResult<Value> {
        let (status, _, body) = self
            .post(
                "/public/admin/v1/conformance/organization-records",
                &json!({"organization":organization}),
            )
            .await?;
        if status != 200 {
            return Err(format!("organization records read failed: {status} {body}").into());
        }
        Ok(body)
    }
}

pub(crate) struct AdminPerson {
    /// The organization fence revision the token was minted under.
    pub(crate) organization_revision: i64,
    pub(crate) id: &'static str,
    pub(crate) subject: &'static str,
    pub(crate) organization: String,
    pub(crate) permissions: Vec<&'static str>,
    pub(crate) key: ProofKey,
}
impl AdminPerson {
    pub(crate) fn token(&self) -> ConformanceResult<String> {
        self.token_with(&|_| {})
    }
    /// A token whose claims `adjust` changes before signing.
    pub(crate) fn token_with(&self, adjust: &dyn Fn(&mut Value)) -> ConformanceResult<String> {
        let mut claims = json!({
            "iss":ISSUER,"aud":[ADMIN_AUDIENCE],"sub":self.id,
            "sid":"019f0000-0000-7000-8000-00000000b2f0",
            "iat":NOW_SECONDS - 10,"exp":NOW_SECONDS + 290,"cnf":{"jkt":self.key.thumbprint()?},
            "token_profile":"person-fleet","organization_id":self.organization,
            "authorization_subject_id":self.subject,"person_revision":1,"organization_revision":self.organization_revision,
            "permission_policy_digest":ADMIN_POLICY,"permissions":self.permissions,
            "second_factor_at":NOW_SECONDS - 60,"second_factor_method":"app",
            "session_expires_at_ms":(NOW_SECONDS + 86_400) * 1000,
            "authorization_snapshot_at_ms":NOW_SECONDS * 1000 - 10_000,
            "authorization_snapshot_valid_until_ms":NOW_SECONDS * 1000 + 200_000
        });
        adjust(&mut claims);
        stamps::sign(
            &json!({"typ":"at+jwt","alg":"EdDSA","kid":"stamp-a"}),
            &claims,
            7,
        )
    }
}
