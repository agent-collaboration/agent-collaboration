//! The native grouping fixture has fixed multi-bucket data and a mounted clock.
use super::*;
use crate::session_picker::SessionGroupFacet;
use agent_session_inventory::sessions::SessionPickerIdentity;

#[test]
fn grouping_fixture_keeps_nine_full_identities_and_the_approved_bucket_table() {
    let request = apply_storyboard_variant(
        provider_filter_storyboard_request().unwrap(),
        Some("r19-grouping"),
    )
    .expect("R19 fixture data must be available");
    let identities = request
        .records
        .iter()
        .map(|record| match &record.identity {
            SessionPickerIdentity::LocalCodex(session_id) => {
                serde_json::json!({"kind":"localCodex","sessionId":session_id})
            }
            SessionPickerIdentity::HostedCodex(target) => {
                serde_json::json!({"kind":"hostedCodex","target":target})
            }
            SessionPickerIdentity::HostedProvider(target) => {
                serde_json::json!({"kind":"hostedProvider","target":target})
            }
            SessionPickerIdentity::NativeClaude(target) => {
                serde_json::json!({"kind":"nativeClaude","target":target})
            }
        })
        .collect::<Vec<_>>();
    assert_eq!(identities,serde_json::json!([
        {"kind":"localCodex","sessionId":"00000000-0000-4000-8000-000000000011"},
        {"kind":"localCodex","sessionId":"00000000-0000-4000-8000-000000000012"},
        {"kind":"hostedProvider","target":{"endpoint":{"serviceId":"00000000-0000-4000-8000-000000000001","endpointId":"claude-fixture"},"sessionId":"00000000-0000-4000-8000-000000000011"}},
        {"kind":"nativeClaude","target":{"endpoint":{"serviceId":"00000000-0000-4000-8000-000000000001","endpointId":"claude-local"},"sessionId":"00000000-0000-4000-8000-000000000014"}},
        {"kind":"hostedProvider","target":{"endpoint":{"serviceId":"00000000-0000-4000-8000-000000000001","endpointId":"cursor-fixture"},"sessionId":"00000000-0000-4000-8000-000000000011"}},
        {"kind":"hostedProvider","target":{"endpoint":{"serviceId":"00000000-0000-4000-8000-000000000001","endpointId":"unknown-fixture"},"sessionId":"unknown-zeta"}},
        {"kind":"hostedCodex","target":{"endpoint":{"serviceId":"00000000-0000-4000-8000-000000000001","endpointId":"codex-source-a"},"sessionId":"00000000-0000-4000-8000-000000000011"}},
        {"kind":"hostedCodex","target":{"endpoint":{"serviceId":"00000000-0000-4000-8000-000000000002","endpointId":"codex-source-a"},"sessionId":"00000000-0000-4000-8000-000000000011"}},
        {"kind":"hostedCodex","target":{"endpoint":{"serviceId":"00000000-0000-4000-8000-000000000002","endpointId":"codex-source-b"},"sessionId":"00000000-0000-4000-8000-000000000011"}},
    ]).as_array().unwrap().clone());
    let model = SessionsPickerModel::new(request, 120);
    let now: chrono::DateTime<chrono::FixedOffset> = "2026-10-31T00:10:00-04:00".parse().unwrap();
    for (facet, expected) in [
        (
            SessionGroupFacet::StartedBy,
            vec![
                ("You", 1),
                ("Messages", 3),
                ("Board", 2),
                ("Schedules and wakes", 1),
                ("Questions and approvals", 1),
                ("Thread activity", 1),
            ],
        ),
        (
            SessionGroupFacet::Day,
            vec![
                ("Today", 2),
                ("Yesterday", 2),
                ("This week", 2),
                ("Older", 3),
            ],
        ),
        (
            SessionGroupFacet::Folder,
            vec![
                ("/fixture/a", 2),
                ("/fixture/b", 1),
                ("/repo/project-a", 4),
                ("/repo/project-b", 1),
                ("Unknown folder", 1),
            ],
        ),
        (
            SessionGroupFacet::Provider,
            vec![
                ("Codex", 5),
                ("Claude Code", 2),
                ("Cursor", 1),
                ("Unclassified", 1),
            ],
        ),
        (
            SessionGroupFacet::Machine,
            vec![
                ("This machine", 6),
                ("Hosted Codex Quartz", 1),
                ("Hosted Codex Cedar", 1),
                ("Hosted Codex Indigo", 1),
            ],
        ),
    ] {
        let groups = model.session_groups(facet, now.with_timezone(&chrono::Utc), now.offset());
        assert_eq!(
            groups
                .iter()
                .map(|group| (group.heading.name.as_str(), group.heading.count))
                .collect::<Vec<_>>(),
            expected,
            "{facet:?}"
        );
    }
}

#[tokio::test]
async fn grouping_fixture_fixed_clock_reaches_the_mounted_day_projection() {
    let directory = tempfile::tempdir().unwrap();
    let mut reached = false;
    let outcome = exercise_at(
        "r19-grouping",
        directory.path(),
        vec![key(KeyCode::F(6)), key(KeyCode::F(6)), key(KeyCode::Home)],
        |canvas, _, model| {
            let frame = canvas.to_string();
            if !reached
                && frame
                    .lines()
                    .any(|line| line.contains("Today") && line.contains("(2)"))
                && frame.contains("❯ Hosted Codex Quartz")
            {
                assert_eq!(model.read().visible_record_len(), 9);
                reached = true;
                return vec![ctrl(KeyCode::Char('d'))].into();
            }
            FixtureStep::default()
        },
        120,
        60,
    )
    .await;
    assert!(reached);
    assert_eq!(outcome, None);
}
