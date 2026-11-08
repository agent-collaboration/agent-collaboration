use super::*;

#[tokio::test]
async fn the_verified_holder_can_answer_an_outstanding_forward_after_reconnecting()
-> ConformanceResult<()> {
    let root = tempfile::tempdir()?;
    let jwks = JwksFixtureServer::start().await?;
    let database = arrange_holder(root.path(), &jwks.url).await?;
    let server = start(root.path(), &jwks.url).await?;
    let proof = async {
        let mut sender = connect(&server, SENDER_CREDENTIAL).await?;
        let mut holder = connect(&server, HOLDER_CREDENTIAL).await?;
        let outstanding =
            forwarded_post_oracles::begin_reconnect(&mut sender, &mut holder, database).await?;
        close(holder).await?;
        let mut replacement = connect(&server, HOLDER_CREDENTIAL).await?;
        forwarded_post_oracles::finish_reconnect(&mut sender, &mut replacement, outstanding)
            .await?;
        close(sender).await?;
        close(replacement).await?;
        Ok(())
    }
    .await;
    let stopped = server.shutdown().await;
    let fixture = jwks.shutdown().await;
    stopped?;
    fixture?;
    proof
}
