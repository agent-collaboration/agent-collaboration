use codex_router_test_support::{FixtureResult, owned_peer_fixture::PeerFixture};
use collaboration_protocol::SessionRef;
use std::{os::unix::fs::DirBuilderExt, path::Path};

#[derive(Default)]
pub struct Recipients {
    pub first: Option<PeerFixture>,
    pub second: Option<PeerFixture>,
    expected_first_frames: Option<u64>,
    expected_second_frames: Option<u64>,
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
