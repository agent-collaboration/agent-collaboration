//! Escaped trail text must page without skipping content or completing a partial range.
#[path = "support/board_fixture.rs"]
#[expect(
    dead_code,
    reason = "This integration target consumes only a subset of the shared fixture's capabilities."
)]
#[expect(
    clippy::indexing_slicing,
    reason = "This test consumes a subset of the shared fixture and retains its fixed JSON construction."
)]
pub mod board_fixture;
use board_fixture::{BoardFixture, TestError, accepted};
use message_board::*;
use serde_json::{Value, json};

#[tokio::test]
#[expect(
    clippy::panic_in_result_fn,
    reason = "Contract assertions panic on a wrong result; Result propagates fixture failures."
)]
async fn escaped_trail_prefixes_preserve_forward_coverage_and_frozen_register_completion()
-> Result<(), TestError> {
    let fixture = BoardFixture::start().await?;
    let (project, _) = fixture.project().await?;
    let mut expected = Vec::new();
    for _ in 0..12 {
        let id = WorkTrailRowId::generate();
        expected.push(json!(id));
        accepted(&fixture.api.call("work_trail_add", fixture.request(&project, json!({"kind":"workTrailAdd","rowId":id,"subject":{"kind":"project"},"text":"\\".repeat(MAX_MESSAGE_TEXT_BYTES),"tags":["decision"],"links":[]}))).await?);
    }
    for descending in [false, true] {
        let mut request = json!({"projectId":project,"selection":if descending { json!({"kind":"decisions","after":null,"before":null,"rangeTop":null}) } else { json!({"kind":"forward","after":{"epoch":1,"position":0}}) },"limit":100,"subject":null,"tags":[],"includeRedacted":false,"reader":{"kind":"anonymous"}});
        let mut observed = Vec::new();
        let mut pages = 0;
        loop {
            let response = fixture.api.call("work_trail_show", request.clone()).await?;
            assert!(
                serde_json::to_vec(&response)?.len()
                    < collaboration_protocol::MAX_CONTROL_FRAME_BYTES
            );
            let page = response
                .pointer("/result/value/page")
                .ok_or_else(|| format!("{response}"))?;
            let rows = page.get("rows").and_then(Value::as_array).ok_or("rows")?;
            observed.extend(rows.iter().filter_map(|row| row.get("rowId")).cloned());
            pages += 1;
            if descending {
                let older = page.get("olderThan").ok_or("older")?;
                if older.is_null() {
                    assert_eq!(page.get("coveredThrough"), page.get("rangeTop"));
                    break;
                }
                assert!(page.get("coveredThrough").is_some_and(Value::is_null));
                request
                    .pointer_mut("/selection/before")
                    .ok_or("before")?
                    .clone_from(older);
                request
                    .pointer_mut("/selection/rangeTop")
                    .ok_or("range top")?
                    .clone_from(page.get("rangeTop").ok_or("range")?);
            } else {
                let covered = page.get("coveredThrough").ok_or("coverage")?;
                if Some(covered) == page.get("highWater") {
                    break;
                }
                request
                    .pointer_mut("/selection/after")
                    .ok_or("after")?
                    .clone_from(covered);
            }
        }
        assert!(pages >= 2, "escaped text must exercise the byte prefix");
        let mut wanted = expected.clone();
        if descending {
            wanted.reverse();
        }
        assert_eq!(
            observed, wanted,
            "byte continuation skipped or repeated trail rows"
        );
    }
    fixture.stop().await
}
