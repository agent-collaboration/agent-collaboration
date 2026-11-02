//! Summary fields and adjacent transitions have a narrower contract than live readback.
use super::super::receipts::{append_summary_pose, write_summary};
use super::*;

fn live_pose(captured: Value) -> Value {
    json!({"mount":1,"browseAccountId":"browse-b","listSelection":"selected-c","captured":captured,
        "scope":"quota:details","focus":"quota:login","published":{"frame":19,"surfaces":[{"cells":[[1,2],[3,4]]}]},
        "rows":[{"accountId":"account-a","loginState":"Required","label":"Alpha","provider":"codex","status":"Ready","enabled":true,"weeklyWindow":"90%","detailsWeeklyWindow":"90% left","resetCredits":"2"}],
        "actions":[{"id":"quota:group-provider","checked":true,"title":"Provider","keys":["F6"],"available":true,"availability":"Available"},{"id":"quota:login","checked":null,"title":"Log in again","keys":["Enter"],"available":true,"availability":"Available"}]})
}
fn compact_pose(captured: Value) -> Value {
    json!({"mount":1,"browseAccountId":"browse-b","listSelection":"selected-c","captured":captured,
        "rows":[{"accountId":"account-a","loginState":"Required"}],
        "actions":[{"id":"quota:group-provider","checked":true},{"id":"quota:login","checked":null}]})
}

#[test]
fn summary_keeps_complete_captured_variants_and_reader_fields_without_mutating_live_readback() {
    // Arrange: every captured variant, including the full Details identity and position.
    for captured in [
        Value::Null,
        json!({"kind":"other"}),
        json!({"kind":"details","accountId":"account-a","provider":"codex","label":"Alpha","position":2}),
        json!({"kind":"options","accountId":"account-a","label":"Alpha"}),
        json!({"kind":"floor","accountId":"account-a","label":"Alpha"}),
    ] {
        let live = live_pose(captured.clone());
        let original = serde_json::to_vec(&live).unwrap();
        let mut poses = Vec::new();
        // Act: the observer's summary-only append boundary.
        append_summary_pose(&mut poses, &live);
        // Assert: exact reader fields and array order; full live bytes remain intact.
        assert_eq!(poses, vec![compact_pose(captured)]);
        assert_eq!(serde_json::to_vec(&live).unwrap(), original);
    }
}

#[test]
fn summary_deduplicates_only_adjacent_compact_equals_and_preserves_every_retained_transition() {
    // Arrange: duplicate redraws differ only in fields the summary does not expose.
    let baseline = live_pose(Value::Null);
    let mut redraw = baseline.clone();
    redraw["published"]["frame"] = json!(20);
    redraw["scope"] = json!("quota:options");
    redraw["focus"] = Value::Null;
    redraw["rows"][0]["label"] = json!("Changed display label");
    redraw["actions"][0]["title"] = json!("Group by Provider");
    let mut poses = Vec::new();
    // Act: redraw equality must be evaluated after projection.
    append_summary_pose(&mut poses, &baseline);
    append_summary_pose(&mut poses, &redraw);
    // Assert: redraw-only differences disappear.
    assert_eq!(poses, vec![compact_pose(Value::Null)]);

    // Arrange / Act / Assert: each retained fact independently produces a transition,
    // followed by the original pose again. A global set would erase these returns.
    for (pointer, changed) in [
        ("/mount", json!(2)),
        ("/browseAccountId", Value::Null),
        ("/listSelection", Value::Null),
        ("/captured", json!({"kind":"other"})),
        ("/rows/0/accountId", json!("other-account")),
        ("/rows/0/loginState", json!("NotRequired")),
        ("/actions/0/id", json!("quota:group-status")),
        ("/actions/0/checked", json!(false)),
        ("/actions/1/checked", json!(true)),
    ] {
        let mut live = baseline.clone();
        *live.pointer_mut(pointer).unwrap() = changed.clone();
        let mut expected = compact_pose(Value::Null);
        *expected.pointer_mut(pointer).unwrap() = changed;
        append_summary_pose(&mut poses, &live);
        assert_eq!(poses.last(), Some(&expected), "lost {pointer}");
        let length = poses.len();
        append_summary_pose(&mut poses, &live);
        assert_eq!(poses.len(), length, "did not deduplicate {pointer}");
        append_summary_pose(&mut poses, &baseline);
        assert_eq!(poses.len(), length + 1, "lost nonadjacent return {pointer}");
        assert_eq!(poses.last(), Some(&compact_pose(Value::Null)));
    }
    assert_eq!(poses.len(), 19);
}

#[test]
#[ignore = "opt-in real-file size comparison over a caller-supplied retained native prefix; no native TUI"]
fn retained_native_receipt_prefix_compaction() {
    // Arrange: retain all top-level facts; replace only publishedPoses via the real projector.
    let input = std::env::var_os("QUOTA_SUMMARY_TIMING_INPUT").expect("explicit workload path");
    let output = std::env::var_os("QUOTA_SUMMARY_COMPACTION_OUTPUT").expect("explicit output path");
    let original_bytes = std::fs::read(input).unwrap();
    let original: Value = serde_json::from_slice(&original_bytes).unwrap();
    let original_poses = original["publishedPoses"].as_array().unwrap();
    let mut compact = Vec::new();
    for pose in original_poses {
        append_summary_pose(&mut compact, pose);
    }
    let mut summary = original.clone();
    summary["publishedPoses"] = json!(compact);
    // Act: same buffered writer and real file sync used by the fixture.
    let mut file = absent_file(std::path::Path::new(&output)).unwrap();
    write_summary(&mut file, &summary).unwrap();
    file.sync_all().unwrap();
    // Assert: complete round trip and every non-pose fact remains unchanged.
    let bytes = std::fs::read(output).unwrap();
    assert_eq!(bytes.last(), Some(&b'\n'));
    assert_eq!(serde_json::from_slice::<Value>(&bytes).unwrap(), summary);
    let mut other_facts = summary.clone();
    other_facts["publishedPoses"] = original["publishedPoses"].clone();
    assert_eq!(other_facts, original);
    assert!(bytes.len() < original_bytes.len());
    eprintln!(
        "NATIVE_PREFIX_COMPACTION before_bytes={} after_bytes={} before_poses={} after_poses={}",
        original_bytes.len(),
        bytes.len(),
        original_poses.len(),
        compact.len()
    );
}
