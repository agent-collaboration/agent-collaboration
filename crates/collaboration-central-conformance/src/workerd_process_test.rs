use super::*;

#[tokio::test]
async fn pre_readiness_exit_returns_the_startup_error_after_settlement() {
    let persistence = tempfile::tempdir().unwrap();
    let settings = WorkerdSettings {
        node: "false".into(),
        miniflare: "unused".into(),
        persistence: persistence.path().to_owned(),
        port: 1,
        jwks_url: "http://127.0.0.1:1/unused".to_owned(),
        holder_answer_timeout: Duration::from_secs(2),
        copy_ack_wait: Duration::from_secs(2),
        relay_peer: "019f0000-0000-7000-8000-000000005240"
            .to_owned()
            .try_into()
            .unwrap(),
    };
    let result = WorkerdProcess::start(&settings).await;
    assert!(
        result.is_err(),
        "pre-readiness child exit must return an error"
    );
}
