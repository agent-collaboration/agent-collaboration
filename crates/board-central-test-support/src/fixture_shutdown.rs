//! Keep the real central fixture's three independently owned cleanup operations together.
use crate::FixtureResult;
use std::future::Future;

pub(crate) async fn finish(
    machine: impl Future<Output = FixtureResult>,
    server: impl Future<Output = FixtureResult>,
    jwks: impl Future<Output = FixtureResult>,
) -> FixtureResult {
    let outcomes = [
        ("machine Access edge", machine.await),
        ("central server", server.await),
        ("JWKS listener", jwks.await),
    ];
    let failures = outcomes
        .into_iter()
        .filter_map(|(owner, result)| result.err().map(|error| format!("{owner}: {error}")))
        .collect::<Vec<_>>();
    if failures.is_empty() {
        Ok(())
    } else {
        Err(failures.join("; ").into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU8, Ordering};

    fn failure<'a>(
        called: &'a AtomicU8,
        mask: u8,
        label: &'a str,
    ) -> impl Future<Output = FixtureResult> + 'a {
        std::future::poll_fn(move |_| {
            called.fetch_or(mask, Ordering::SeqCst);
            std::task::Poll::Ready(Err(label.to_owned().into()))
        })
    }

    #[tokio::test]
    async fn every_cleanup_runs_and_retains_all_sibling_errors() {
        let called = AtomicU8::new(0);
        let result = finish(
            failure(&called, 1, "machine edge cleanup failed"),
            failure(&called, 2, "central server cleanup failed"),
            failure(&called, 4, "JWKS cleanup failed"),
        )
        .await;
        assert_eq!(
            called.load(Ordering::SeqCst),
            7,
            "all three owners must settle"
        );
        let error = result
            .expect_err("cleanup failures must be retained")
            .to_string();
        for label in [
            "machine edge cleanup failed",
            "central server cleanup failed",
            "JWKS cleanup failed",
        ] {
            assert!(
                error.contains(label),
                "missing sibling cleanup error: {error}"
            );
        }
    }

    #[tokio::test]
    #[expect(
        clippy::panic_in_result_fn,
        reason = "Contract assertions fail the test while fixture errors propagate."
    )] // Literal assertions observe the owned listener after a real setup failure.
    async fn failed_central_start_joins_the_real_jwks_listener() -> FixtureResult {
        let root = tempfile::tempdir()?;
        let blocked = root.path().join("central-is-a-file");
        std::fs::write(&blocked, b"owned startup failure fixture")?;
        let origin = message_board::ActorOrigin {
            installation: "019f0000-0000-7000-8000-00000000bc01"
                .to_owned()
                .try_into()?,
            machine: "central cleanup fixture".to_owned().try_into()?,
        };
        let mut observed = None;
        let result = crate::TestCentral::start_observing_jwks(
            &blocked,
            &origin,
            "019f0000-0000-7000-8000-00000000bc02"
                .to_owned()
                .try_into()?,
            |url| observed = Some(url.to_owned()),
        )
        .await;
        assert!(
            result.is_err(),
            "file in place of central root must refuse startup"
        );
        let url = reqwest::Url::parse(
            &observed.ok_or("JWKS was not started before the controlled failure")?,
        )?;
        let endpoint = (
            url.host_str().ok_or("JWKS host absent")?,
            url.port().ok_or("JWKS port absent")?,
        );
        let connection =
            tokio::time::timeout(crate::BOUND, tokio::net::TcpStream::connect(endpoint)).await?;
        assert!(
            connection.is_err(),
            "failed fixture setup left its actual JWKS listener alive"
        );
        Ok(())
    }
}
