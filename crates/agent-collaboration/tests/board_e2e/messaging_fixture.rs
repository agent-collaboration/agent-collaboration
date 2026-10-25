//! Rule cases borrow the shared Host pair and own only their peer and fault holds.
use super::owned_host::StoppedHost;
use super::{central_pair, fixture::BoardFixture};
use board_central_test_support::{DispatchCallSelector, ObjectAddress, RuntimeControl};
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
    held_sources: Vec<ObjectAddress>,
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

    pub async fn stop_peer(&mut self) -> FixtureResult {
        self.peer
            .take()
            .ok_or("rule peer is already stopped")?
            .shutdown()
            .await
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

    pub fn sender_connection(&self) -> FixtureResult<ObjectAddress> {
        let service = self
            .sender
            .installation
            .as_ref()
            .ok_or("sender identity absent")?
            .service_id
            .clone();
        Ok(ObjectAddress::Connection(String::from(service).try_into()?))
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

    pub async fn hold_object_call(
        &mut self,
        source: ObjectAddress,
        destination: ObjectAddress,
        selected_call: Option<DispatchCallSelector>,
    ) -> FixtureResult {
        if !self.held_sources.contains(&source) {
            self.held_sources.push(source.clone());
        }
        let command = match selected_call {
            Some(call) => RuntimeControl::HoldNextSelectedDispatch { destination, call },
            None => RuntimeControl::HoldNextDispatch { destination },
        };
        self.control(source, command).await?;
        Ok(())
    }

    pub async fn resume_object_calls(&mut self, source: ObjectAddress) -> FixtureResult {
        self.control(source.clone(), RuntimeControl::ResumeDispatch)
            .await?;
        self.held_sources.retain(|held| held != &source);
        Ok(())
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
        for source in std::mem::take(&mut self.held_sources) {
            if let Err(error) = self.control(source, RuntimeControl::ResumeDispatch).await {
                failures.push(format!("resume held object: {error}"));
            }
        }
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
                held_sources: Vec::new(),
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
