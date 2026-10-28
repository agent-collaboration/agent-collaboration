//! Rule cases borrow the shared Host pair and own only their peer and fault holds.
use super::owned_host::StoppedHost;
use super::{central_pair, fixture::BoardFixture};
use board_central_test_support::{ObjectAddress, RuntimeControl};
use codex_router_test_support::{
    FixtureResult, owned_peer_fixture::PeerFixture, scenario_finalization::run_scenario,
};
use collaboration_protocol::{EndpointRef, SessionRef};
use serde_json::{Value, json};
use std::{ops::AsyncFnOnce, os::unix::fs::DirBuilderExt};

pub struct MessagingCase<'fixture> {
    pub sender: &'fixture mut BoardFixture,
    pub recipient: &'fixture mut central_pair::SecondHost,
    peer: Option<PeerFixture>,
    target: Option<SessionRef>,
}

impl MessagingCase<'_> {
    pub async fn start_peer(&mut self) -> FixtureResult {
        let registry = self.recipient.directory.join("absent-peer-registry");
        std::fs::DirBuilder::new()
            .recursive(true)
            .mode(0o700)
            .create(&registry)?;
        let endpoint: EndpointRef = serde_json::from_value(json!({
            "serviceId":self.recipient.service_id()?,"endpointId":"claude-local"
        }))?;
        let peer = PeerFixture::start_in_registry(&registry, &endpoint).await?;
        if let Some(target) = &self.target {
            assert_eq!(&peer.target, target);
        }
        self.target = Some(peer.target.clone());
        self.peer = Some(peer);
        Ok(())
    }

    pub fn peer(&mut self) -> FixtureResult<&mut PeerFixture> {
        self.peer.as_mut().ok_or_else(|| "rule peer absent".into())
    }

    pub fn recipient_target(&self) -> FixtureResult<SessionRef> {
        self.target
            .clone()
            .ok_or_else(|| "rule target absent".into())
    }

    pub fn mailbox_address(&self) -> FixtureResult<ObjectAddress> {
        Ok(ObjectAddress::Mailbox(
            self.recipient.service_id()?.try_into()?,
        ))
    }

    pub async fn control(
        &self,
        source: ObjectAddress,
        command: RuntimeControl,
    ) -> FixtureResult<Value> {
        Ok(serde_json::to_value(
            central_pair::central(self.sender)?
                .runtime_control(source, command)
                .await?,
        )?)
    }

    pub async fn stop_sender_for_readback(&mut self) -> FixtureResult<StoppedHost> {
        self.sender
            .host
            .take()
            .ok_or("sender Host is already stopped")?
            .stop_for_readback()
            .await
    }

    async fn finalize(mut self) -> FixtureResult {
        let mut failures = Vec::new();
        if let Some(peer) = self.peer.take()
            && let Err(error) = peer.shutdown().await
        {
            failures.push(format!("rule peer: {error}"));
        }
        if failures.is_empty() {
            Ok(())
        } else {
            Err(failures.join("; ").into())
        }
    }
}

pub async fn mail_scenario<F>(case: F) -> FixtureResult
where
    F: for<'borrow, 'fixture> AsyncFnOnce(&'borrow mut MessagingCase<'fixture>) -> FixtureResult,
{
    central_pair::scenario(async move |sender, recipient| {
        run_scenario(
            MessagingCase {
                sender,
                recipient,
                peer: None,
                target: None,
            },
            async move |scope: &mut MessagingCase<'_>| {
                scope.start_peer().await?;
                case(scope).await
            },
            MessagingCase::finalize,
            || Ok(()),
        )
        .await
    })
    .await
}
