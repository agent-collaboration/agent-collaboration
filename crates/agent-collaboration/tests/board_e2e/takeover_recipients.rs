//! Real private Unix peers observe injections; finalization always joins both peers.
use super::{
    acp_recipient::AcpRecipient,
    central_pair::SecondHost,
    fixture::{BoardFixture, identity},
    owned_host::BoardHost,
};
use codex_router_test_support::{FixtureResult, owned_peer_fixture::PeerFixture};
use collaboration_protocol::SessionRef;
use owned_process_test_support::scenario_finalization::run_scenario;
use serde_json::json;
use std::{ops::AsyncFnOnce, os::unix::fs::DirBuilderExt, path::Path};

#[derive(Default)]
pub struct Recipients {
    pub first: Option<PeerFixture>,
    pub second: Option<PeerFixture>,
    expected_first_frames: Option<u64>,
    expected_second_frames: Option<u64>,
}

pub async fn session(host: &BoardHost) -> FixtureResult<SessionRef> {
    let identity = host.mcp("installation_show", json!({})).await?;
    Ok(serde_json::from_value(json!({"endpoint":{
        "serviceId":identity.get("serviceId").ok_or("service identity absent")?,
        "endpointId":"claude-local"},"sessionId":"fixture-session"}))?)
}

async fn start(directory: &Path, target: &SessionRef) -> FixtureResult<PeerFixture> {
    let registry = directory.join("absent-peer-registry");
    if !registry.exists() {
        std::fs::DirBuilder::new().mode(0o700).create(&registry)?;
    }
    let peer = PeerFixture::start_in_registry(&registry, &target.endpoint).await?;
    assert_eq!(&peer.target, target);
    Ok(peer)
}

impl Recipients {
    pub async fn start_first(
        &mut self,
        fixture: &BoardFixture,
        expected_frames: u64,
    ) -> FixtureResult {
        if self.first.is_some() {
            return Err("first peer already exists".into());
        }
        self.first = Some(start(&fixture.directory, &session(fixture.host()?).await?).await?);
        self.expected_first_frames = Some(expected_frames);
        Ok(())
    }

    pub async fn start_second(
        &mut self,
        second: &SecondHost,
        expected_frames: u64,
    ) -> FixtureResult {
        if self.second.is_some() {
            return Err("second peer already exists".into());
        }
        self.second = Some(start(&second.directory, &session(second.host()?).await?).await?);
        self.expected_second_frames = Some(expected_frames);
        Ok(())
    }

    pub fn second(&mut self) -> FixtureResult<&mut PeerFixture> {
        self.second
            .as_mut()
            .ok_or_else(|| "second peer absent".into())
    }

    pub fn first(&mut self) -> FixtureResult<&mut PeerFixture> {
        self.first
            .as_mut()
            .ok_or_else(|| "first peer absent".into())
    }

    async fn shutdown(mut self) -> FixtureResult {
        // The inner control finalizer has joined both Host effect owners.
        // The common fixture still owns the peers' socket directories.
        let first_count = assert_total(&mut self.first, self.expected_first_frames).await;
        let second_count = assert_total(&mut self.second, self.expected_second_frames).await;
        let first = match self.first.take() {
            Some(peer) => peer.shutdown().await,
            None => Ok(()),
        };
        let second = match self.second.take() {
            Some(peer) => peer.shutdown().await,
            None => Ok(()),
        };
        let failures = [
            ("first final count", first_count),
            ("second final count", second_count),
            ("first peer join", first),
            ("second peer join", second),
        ]
        .into_iter()
        .filter_map(|(owner, result)| result.err().map(|error| format!("{owner}: {error}")))
        .collect::<Vec<_>>();
        if failures.is_empty() {
            Ok(())
        } else {
            Err(failures.join("; ").into())
        }
    }
}

async fn assert_total(peer: &mut Option<PeerFixture>, expected: Option<u64>) -> FixtureResult {
    match (peer, expected) {
        (Some(peer), Some(expected)) => {
            let actual = peer.count_after_barrier().await?;
            if actual == expected {
                Ok(())
            } else {
                Err(format!("expected {expected} recipient frames, observed {actual}").into())
            }
        }
        (None, None) => Ok(()),
        _ => Err("recipient peer and its expected total disagree".into()),
    }
}

pub async fn scenario<F>(
    fixture: &mut BoardFixture,
    second: &mut SecondHost,
    case: F,
) -> FixtureResult
where
    F: for<'a> AsyncFnOnce(
        &'a mut BoardFixture,
        &'a mut SecondHost,
        &'a mut Recipients,
    ) -> FixtureResult,
{
    run_scenario(
        Recipients::default(),
        async move |peers: &mut Recipients| case(fixture, second, peers).await,
        Recipients::shutdown,
        || Ok(()),
    )
    .await
}

/// Recipients the Router holds follow mail for and later loads: a Cursor-endpoint session
/// on the fixture ACP provider each armed Host runs. A recipient is absent while its
/// Host's provider has not loaded the session, and present once it is loaded.
#[derive(Default)]
pub struct AcpRecipients {
    first: Option<AcpRecipient>,
    second: Option<AcpRecipient>,
    expected_first_frames: Option<u64>,
    expected_second_frames: Option<u64>,
}

impl AcpRecipients {
    /// Names the recipient on the first Host, absent.
    pub async fn arm_first(&mut self, fixture: &mut BoardFixture) -> FixtureResult<SessionRef> {
        if self.first.is_some() {
            return Err("first recipient already exists".into());
        }
        let service_id = String::from(
            fixture
                .installation
                .as_ref()
                .ok_or("first identity absent")?
                .service_id
                .clone(),
        );
        // Absent, with a record: the Host restarts with the provider, one load gives the
        // session its record, and a second restart leaves it unloaded.
        let recipient = AcpRecipient::arm(&fixture.directory, &service_id, &identity())?;
        fixture.stop_host().await?;
        fixture.reopen_host().await?;
        recipient.load(fixture.host()?).await?;
        fixture.stop_host().await?;
        fixture.reopen_host().await?;
        let target = recipient.target.clone();
        self.first = Some(recipient);
        Ok(target)
    }

    /// Names the recipient on the second Host, absent.
    pub async fn arm_second(&mut self, second: &mut SecondHost) -> FixtureResult<SessionRef> {
        if self.second.is_some() {
            return Err("second recipient already exists".into());
        }
        let service_id = second.service_id()?;
        let recipient = AcpRecipient::arm(&second.directory, &service_id, &identity())?;
        second.stop_host().await?;
        second.reopen_host().await?;
        recipient.load(second.host()?).await?;
        second.stop_host().await?;
        second.reopen_host().await?;
        let target = recipient.target.clone();
        self.second = Some(recipient);
        Ok(target)
    }

    /// The first recipient comes back: its Host's provider loads the session.
    pub async fn start_first(
        &mut self,
        fixture: &BoardFixture,
        expected_frames: u64,
    ) -> FixtureResult {
        self.first()?.come_back(fixture.host()?).await?;
        self.expected_first_frames = Some(expected_frames);
        Ok(())
    }

    /// The second recipient comes back: its Host's provider loads the session.
    pub async fn start_second(
        &mut self,
        second: &SecondHost,
        expected_frames: u64,
    ) -> FixtureResult {
        self.second()?.come_back(second.host()?).await?;
        self.expected_second_frames = Some(expected_frames);
        Ok(())
    }

    pub fn first(&self) -> FixtureResult<&AcpRecipient> {
        self.first
            .as_ref()
            .ok_or_else(|| "first recipient absent".into())
    }

    pub fn second(&self) -> FixtureResult<&AcpRecipient> {
        self.second
            .as_ref()
            .ok_or_else(|| "second recipient absent".into())
    }

    #[expect(
        clippy::unused_async,
        reason = "The scenario finalizer it serves is asynchronous."
    )]
    async fn shutdown(self) -> FixtureResult {
        let failures = [
            (
                "first final count",
                acp_total(self.first.as_ref(), self.expected_first_frames),
            ),
            (
                "second final count",
                acp_total(self.second.as_ref(), self.expected_second_frames),
            ),
        ]
        .into_iter()
        .filter_map(|(owner, result)| result.err().map(|error| format!("{owner}: {error}")))
        .collect::<Vec<_>>();
        if failures.is_empty() {
            Ok(())
        } else {
            Err(failures.join("; ").into())
        }
    }
}

fn acp_total(recipient: Option<&AcpRecipient>, expected: Option<u64>) -> FixtureResult {
    match (recipient, expected) {
        (Some(recipient), Some(expected)) => {
            let actual = recipient.count()?;
            if actual == expected {
                Ok(())
            } else {
                Err(format!("expected {expected} recipient frames, observed {actual}").into())
            }
        }
        // Armed but never brought back: nothing may have reached it.
        (Some(recipient), None) => match recipient.count()? {
            0 => Ok(()),
            actual => Err(format!("an absent recipient received {actual} frames").into()),
        },
        (None, None) => Ok(()),
        (None, Some(_)) => Err("recipient and its expected total disagree".into()),
    }
}

pub async fn acp_scenario<F>(
    fixture: &mut BoardFixture,
    second: &mut SecondHost,
    case: F,
) -> FixtureResult
where
    F: for<'a> AsyncFnOnce(
        &'a mut BoardFixture,
        &'a mut SecondHost,
        &'a mut AcpRecipients,
    ) -> FixtureResult,
{
    run_scenario(
        AcpRecipients::default(),
        async move |recipients: &mut AcpRecipients| case(fixture, second, recipients).await,
        AcpRecipients::shutdown,
        || Ok(()),
    )
    .await
}
