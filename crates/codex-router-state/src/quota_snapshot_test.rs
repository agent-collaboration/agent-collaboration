use super::QuotaSnapshotSource;

#[test]
fn quota_snapshot_sources_round_trip_existing_and_claude_endpoint_tags() {
    for (source, tag) in [
        (QuotaSnapshotSource::MockEndpoint, "mock_endpoint"),
        (QuotaSnapshotSource::OpenAiEndpoint, "openai_endpoint"),
        (QuotaSnapshotSource::ClaudeEndpoint, "claude_endpoint"),
        (
            QuotaSnapshotSource::CredentialMutation,
            "credential_mutation",
        ),
    ] {
        assert_eq!(source.as_str(), tag);
        assert_eq!(QuotaSnapshotSource::parse(tag), Some(source));
    }
    assert_eq!(QuotaSnapshotSource::parse("unknown_endpoint"), None);
}
