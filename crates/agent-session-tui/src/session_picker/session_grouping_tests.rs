use super::*;
use crate::session_picker::test_support::picker_record;
use agent_session_inventory::session_provider_family::SessionProviderFamily;
use collaboration_client::protocol::ProviderKind;

fn now() -> DateTime<Utc> {
    "2026-10-31T16:00:00Z".parse().unwrap()
}
fn groups(facet: SessionGroupFacet, rows: &[SessionPickerRecord]) -> Vec<SessionGroup> {
    compute_session_groups(facet, rows, now(), &Utc, &[])
}
fn headings(groups: &[SessionGroup]) -> Vec<(&str, usize)> {
    groups
        .iter()
        .map(|group| (group.heading.name.as_str(), group.heading.count))
        .collect()
}
fn record(id: &str) -> SessionPickerRecord {
    picker_record(id, "Owner's title", "/repo/project", "provider", "cli")
}

#[test]
fn session_grouping_facets_have_fixed_names_order_and_strict_stored_values() {
    assert_eq!(
        SessionGroupFacet::CHOICES.map(SessionGroupFacet::label),
        ["None", "Started by", "Day", "Folder", "Provider", "Machine"]
    );
    for facet in SessionGroupFacet::CHOICES {
        assert_eq!(
            SessionGroupFacet::from_stored_value(facet.stored_value()),
            facet
        );
    }
    for unknown in ["", "removed", "DAY", "day\n", "\0"] {
        assert_eq!(
            SessionGroupFacet::from_stored_value(unknown),
            SessionGroupFacet::None
        );
    }
}

#[test]
fn session_grouping_none_and_empty_rows_have_no_headings() {
    assert!(groups(SessionGroupFacet::None, &[record("one")]).is_empty());
    for facet in SessionGroupFacet::CHOICES {
        assert!(groups(facet, &[]).is_empty());
    }
}

#[test]
fn session_grouping_started_by_reads_complete_first_messages_in_fixed_order() {
    let headers = [
        "🧵 Router: subscription expired @Sunbook · scope",
        "❓ Router question @Sunbook · Main asks",
        "🗓 Router schedule 018f47d2 @Sunbook · run 018f47d2",
        "📋 Router: board notification @Sunbook",
        "✉️ Main @Sunbook → you",
        "🧵 Router: new thread activity @Sunbook · 1 thread · 2 messages",
        "❓ Router approval @Sunbook · Main asks",
        "⏰ Router wake @Sunbook",
    ];
    let mut rows: Vec<_> = headers.iter().enumerate().map(|(index, header)| {
        let mut row = record(&format!("push-{index}"));
        row.first_user_message = format!("{header} · router://018f47d2-24d5-7a68-b9ec-6f759c39458f/push/018f47d2-24d5-7a68-b9ec-6f759c39458f");
        row
    }).collect();
    let mut owner = record("owner");
    owner.title = headers[4].to_owned();
    owner.full_title = rows[4].first_user_message.clone();
    owner.preview = Some(rows[4].first_user_message.clone());
    owner.first_user_message = "✉️ notes".to_owned();
    rows.push(owner);
    let result = groups(SessionGroupFacet::StartedBy, &rows);
    assert_eq!(
        headings(&result),
        [
            ("You", 1),
            ("Messages", 1),
            ("Board", 1),
            ("Schedules and wakes", 2),
            ("Questions and approvals", 2),
            ("Thread activity", 2)
        ]
    );
    assert_eq!(result[0].keys, [rows[8].identity.clone()]);
    assert_eq!(
        result[3].keys,
        [rows[2].identity.clone(), rows[7].identity.clone()]
    );
}

#[test]
fn session_grouping_provider_uses_client_family_order_and_keeps_input_sort() {
    let families = [
        SessionProviderFamily::Unclassified,
        SessionProviderFamily::External(ProviderKind::Cursor),
        SessionProviderFamily::Codex,
        SessionProviderFamily::External(ProviderKind::ClaudeCode),
        SessionProviderFamily::Codex,
    ];
    let rows: Vec<_> = families
        .into_iter()
        .enumerate()
        .map(|(index, family)| {
            let mut row = record(&format!("family-{index}"));
            row.provider_family = family;
            row
        })
        .collect();
    let result = groups(SessionGroupFacet::Provider, &rows);
    assert_eq!(
        headings(&result),
        [
            ("Codex", 2),
            ("Claude Code", 1),
            ("Cursor", 1),
            ("Unclassified", 1)
        ]
    );
    assert_eq!(
        result[0].keys,
        [rows[2].identity.clone(), rows[4].identity.clone()]
    );
}

#[test]
fn session_grouping_folders_keep_distinct_paths_and_omit_empty_groups() {
    let mut rows = vec![record("z"), record("a"), record("unknown")];
    rows[0].cwd = Some("/z/project".to_owned());
    rows[0].normalized_cwd = Some("/z/project".to_owned());
    rows[1].cwd = Some("/a/project".to_owned());
    rows[1].normalized_cwd = Some("/a/project".to_owned());
    rows[2].cwd = None;
    rows[2].normalized_cwd = None;
    let result = groups(SessionGroupFacet::Folder, &rows);
    assert_eq!(
        headings(&result),
        [("/a/project", 1), ("/z/project", 1), ("Unknown folder", 1)]
    );
}

#[test]
fn session_grouping_machine_keeps_source_qualified_keys() {
    let mut rows = vec![record("same"), record("same")];
    rows[0].source_context = Some(PickerSourceContext::LocalCodex);
    rows[1].source_context = Some(PickerSourceContext::DefaultHosted);
    let target = serde_json::from_value(serde_json::json!({"endpoint":{"serviceId":"00000000-0000-4000-8000-000000000001","endpointId":"codex-local"},"sessionId":"same"})).unwrap();
    rows[1].identity = SessionPickerIdentity::HostedCodex(target);
    let result = compute_session_groups(
        SessionGroupFacet::Machine,
        &rows,
        now(),
        &Utc,
        &[
            PickerSourceContext::DefaultHosted,
            PickerSourceContext::LocalCodex,
        ],
    );
    assert_eq!(headings(&result), [("This machine", 1), ("Local Codex", 1)]);
    assert_eq!(result[0].keys, [rows[1].identity.clone()]);
    assert_eq!(result[1].keys, [rows[0].identity.clone()]);
}

#[test]
fn session_grouping_day_uses_local_calendar_midnight_and_monday_boundaries() {
    let zone = chrono_tz::America::Toronto;
    let timestamps = [
        "2026-10-31T04:00:00Z",
        "2026-10-31T03:59:59Z",
        "2026-10-30T03:59:59Z",
        "2026-10-26T04:00:00Z",
        "2026-10-26T03:59:59Z",
    ];
    let rows: Vec<_> = timestamps
        .into_iter()
        .enumerate()
        .map(|(index, timestamp)| {
            let mut row = record(&format!("day-{index}"));
            row.recency_at_ms = Some(
                timestamp
                    .parse::<DateTime<Utc>>()
                    .unwrap()
                    .timestamp_millis(),
            );
            row
        })
        .collect();
    let result = compute_session_groups(SessionGroupFacet::Day, &rows, now(), &zone, &[]);
    assert_eq!(
        headings(&result),
        [
            ("Today", 1),
            ("Yesterday", 1),
            ("This week", 2),
            ("Older", 1)
        ]
    );
    assert_eq!(
        result[2].keys,
        [rows[2].identity.clone(), rows[3].identity.clone()]
    );
}

#[test]
fn session_grouping_day_handles_dst_and_unknown_recency_without_elapsed_day_math() {
    let zone = chrono_tz::America::Toronto;
    let now = "2026-11-02T05:10:00Z".parse().unwrap();
    let mut rows = vec![record("yesterday"), record("missing"), record("invalid")];
    rows[0].recency_at_ms = Some(
        "2026-11-01T04:05:00Z"
            .parse::<DateTime<Utc>>()
            .unwrap()
            .timestamp_millis(),
    );
    rows[1].recency_at_ms = None;
    rows[2].recency_at_ms = Some(i64::MAX);
    let result = compute_session_groups(SessionGroupFacet::Day, &rows, now, &zone, &[]);
    assert_eq!(headings(&result), [("Yesterday", 1), ("Older", 2)]);
}

#[test]
fn session_grouping_model_projects_only_filtered_rows_in_the_existing_sort() {
    use crate::session_picker::{picker_model::SessionsPickerModel, test_support::picker_request};
    let mut request = picker_request();
    request.records = [record("old-keep"), record("hidden"), record("new-keep")].to_vec();
    for (index, row) in request.records.iter_mut().enumerate() {
        row.title = row.session_id.clone();
        row.full_title = row.title.clone();
        row.first_user_message = row.title.clone();
        row.preview = None;
        row.cwd = Some("/repo/project-a".to_owned());
        row.normalized_cwd = row.cwd.clone();
        row.recency_at_ms = Some([100, 300, 200][index]);
    }
    let expected = [
        request.records[2].identity.clone(),
        request.records[0].identity.clone(),
    ];
    let mut model = SessionsPickerModel::new(request, 120);
    model.append_search_text("keep");
    let selected = model.focused_identity().cloned();
    let result = model.session_groups(SessionGroupFacet::Provider, now(), &Utc);
    assert_eq!(headings(&result), [("Codex", 2)]);
    assert_eq!(result[0].keys, expected);
    assert_eq!(model.focused_identity(), selected.as_ref());
}
