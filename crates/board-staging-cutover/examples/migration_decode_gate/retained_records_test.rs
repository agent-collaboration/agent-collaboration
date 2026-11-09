//! Each receiver record family the migration carries in a Copy, decoded by the
//! receiver's own readers through `inspect_retained_records`: a record the
//! receiver wrote passes, and the same record damaged is refused, naming its
//! table. Records are built from the protocol's own types, for the fixture's
//! first Copy (authoritative, fence 3, acked {3, 17}).

use crate::gate::{GateReport, Outcome, run_gate};
use crate::gate_test::{add_copy_row, edited, integer, text};
use fleet_sync_protocol::{
    CommitSequence, CopyOrigin, CopyRole, CopyStatus, CreateCopy, DatabaseId, Epoch,
    FenceExpectation, FenceRequest, FenceResult, InstallId, PageDigest, PeerId, Piece,
    PieceContents, PieceDigest, ReplicatedTransaction, ReplicationPosition, RowChange,
    SchemaIdentity, SchemaSupport, SeedAttemptId, SeedId, SeedPage, SnapshotPage, SqlValue,
    TransactionKey, TransactionPart, TransferId,
};
use fleet_sync_receiver::SnapshotInstall;
use serde_json::Value;

fn uuid(value: &str) -> uuid::Uuid {
    value.parse().expect("fixture UUID")
}
fn database() -> DatabaseId {
    DatabaseId::new(uuid("01a1c070-0000-7000-8000-000000000001")).expect("database")
}
fn holder() -> PeerId {
    PeerId::new(uuid("01a1c070-0000-7000-8000-00000000b001")).expect("holder")
}
fn position(epoch: u64, sequence: u64) -> ReplicationPosition {
    ReplicationPosition::new(
        Epoch::new(epoch).expect("epoch"),
        CommitSequence::new(sequence),
    )
}
fn schema() -> SchemaIdentity {
    SchemaIdentity::new(1, "schema-fingerprint".into()).expect("schema")
}
fn json(value: &impl serde::Serialize) -> String {
    serde_json::to_string(value).expect("record serializes")
}
fn sql_null() -> Value {
    serde_json::json!({ "kind": "null" })
}

/// The candidate with these rows added to the first Copy.
fn with_rows(rows: Vec<(&'static str, Value)>) -> String {
    edited(move |candidate| {
        for (table, row) in rows {
            add_copy_row(candidate, table, row);
        }
    })
}

/// Asserts the control passes and the damaged candidate is refused at `table`.
async fn passes_and_damage_is_refused(
    control: Vec<(&'static str, Value)>,
    damaged: Vec<(&'static str, Value)>,
    table: &str,
) {
    // Act
    let control = run_gate(&with_rows(control)).await;
    let damaged = run_gate(&with_rows(damaged)).await;

    // Assert
    assert_eq!(control.outcome, Outcome::Pass, "{control:?}");
    assert_refused_at(&damaged, table);
}

fn assert_refused_at(report: &GateReport, table: &str) {
    assert_eq!(report.outcome, Outcome::Refused, "{report:?}");
    assert!(
        report
            .refusals
            .iter()
            .any(|refusal| refusal.object.starts_with("copy ")
                && refusal.row == table
                && refusal.reason.contains("main's receiver reader refuses")),
        "expected a refusal at {table}: {:?}",
        report.refusals
    );
}

#[tokio::test]
async fn a_creation_request_decodes_and_a_malformed_one_is_refused() {
    // Arrange
    let request = CreateCopy {
        database: database(),
        epoch: Epoch::new(1).expect("epoch"),
        holder: Some(holder()),
        origin: CopyOrigin::Empty,
    };
    let row = |request: String| {
        vec![(
            "_fleet_sync_creation",
            serde_json::json!([integer(1), text(&request)]),
        )]
    };

    // Act / Assert
    passes_and_damage_is_refused(
        row(json(&request)),
        row("{}".into()),
        "_fleet_sync_creation",
    )
    .await;
}

/// The fence that ended `old_epoch` at `acked` and granted the next epoch, as
/// the receiver stores it: its request, and its successful result.
fn fence(
    transfer: &str,
    old_epoch: u64,
    acked: ReplicationPosition,
) -> (FenceRequest, FenceResult) {
    let request = FenceRequest {
        database: database(),
        transfer_id: TransferId::new(uuid(transfer)).expect("transfer"),
        new_epoch: Epoch::new(old_epoch + 1).expect("epoch"),
        expect: FenceExpectation {
            epoch: Epoch::new(old_epoch).expect("epoch"),
            acked,
        },
        target_schema: Some(SchemaSupport { max_version: 1 }),
        holder: holder(),
    };
    let result = FenceResult::Fenced {
        old_epoch: Epoch::new(old_epoch).expect("epoch"),
        new_epoch: Epoch::new(old_epoch + 1).expect("epoch"),
        start: acked,
    };
    (request, result)
}
fn fence_row(
    stored_under: &FenceRequest,
    request: &FenceRequest,
    result: &FenceResult,
) -> (&'static str, Value) {
    (
        "_fleet_sync_fence_receipts",
        serde_json::json!([
            text(&json(&stored_under.transfer_id)),
            text(&json(request)),
            text(&stored_under.expect.epoch.value().to_string()),
            sql_null(),
            text(&json(result))
        ]),
    )
}

#[tokio::test]
async fn fence_receipts_decode_at_their_own_addresses_and_a_damaged_or_aliased_one_is_refused() {
    // Arrange: A ends epoch 1 at {1, 4} and grants 2; B ends 2 at {2, 5} and grants 3.
    let (a, a_result) = fence("01a1c070-0000-7000-8000-0000000f0001", 1, position(1, 4));
    let (b, b_result) = fence("01a1c070-0000-7000-8000-0000000f0002", 2, position(2, 5));
    let not_advanced = FenceResult::FenceNotAdvanced {
        current_epoch: Epoch::new(2).expect("epoch"),
    };
    let control = vec![fence_row(&a, &a, &a_result), fence_row(&b, &b, &b_result)];
    // A's result is no longer a successful fence.
    let damaged = vec![
        fence_row(&a, &a, &not_advanced),
        fence_row(&b, &b, &b_result),
    ];
    // A holds B's request, which B's own address accepts; only A's address refuses it.
    let aliased = vec![fence_row(&a, &b, &a_result), fence_row(&b, &b, &b_result)];
    let row_a = json(&a.transfer_id);

    // Act
    let control = run_gate(&with_rows(control)).await;
    let damaged = run_gate(&with_rows(damaged)).await;
    let aliased = run_gate(&with_rows(aliased)).await;

    // Assert
    assert_eq!(control.outcome, Outcome::Pass, "{control:?}");
    for (name, report) in [("damaged", &damaged), ("aliased", &aliased)] {
        assert_refused_at(report, "_fleet_sync_fence_receipts");
        assert!(
            report
                .refusals
                .iter()
                .any(|refusal| refusal.row == "_fleet_sync_fence_receipts"
                    && refusal.reason.contains(&row_a)),
            "{name} must name row A: {:?}",
            report.refusals
        );
    }
}

fn seed_record(generation: u64) -> (&'static str, Value) {
    let record = serde_json::json!({ "database": database(), "seed": SeedId { generation }, "published": null });
    (
        "_fleet_sync_seed",
        serde_json::json!([integer(1), text(&record.to_string())]),
    )
}

#[tokio::test]
async fn a_seed_record_decodes_and_a_generation_zero_one_is_refused() {
    passes_and_damage_is_refused(
        vec![seed_record(1)],
        vec![seed_record(0)],
        "_fleet_sync_seed",
    )
    .await;
}

fn snapshot_page() -> SnapshotPage {
    SnapshotPage {
        database: database(),
        position: position(3, 17),
        schema: schema(),
        schema_steps: Vec::new(),
        table: "records".into(),
        rows: Vec::new(),
        next: None,
    }
}

#[tokio::test]
async fn a_seed_page_decodes_and_one_with_another_digest_is_refused() {
    // Arrange: generation 1's first page; the damage files it under another page's digest.
    let contents = snapshot_page();
    let digest = PageDigest::of(&contents).expect("page digest");
    let page = SeedPage {
        seed: SeedId { generation: 1 },
        page: 0,
        digest: digest.clone(),
        contents,
    };
    let row = |digest: &str| {
        vec![
            seed_record(1),
            (
                "_fleet_sync_seed_pages",
                serde_json::json!([text("1"), integer(0), text(digest), text(&json(&page))]),
            ),
        ]
    };
    let mut other = snapshot_page();
    other
        .schema_steps
        .push("CREATE TABLE records(id TEXT PRIMARY KEY)".into());
    let other = PageDigest::of(&other).expect("other digest");

    // Act / Assert
    passes_and_damage_is_refused(
        row(digest.as_str()),
        row(other.as_str()),
        "_fleet_sync_seed_pages",
    )
    .await;
}

#[tokio::test]
async fn a_seed_attempt_decodes_and_a_malformed_one_is_refused() {
    // Arrange
    let attempt =
        SeedAttemptId::new(uuid("01a1c070-0000-7000-8000-0000000a0001")).expect("attempt");
    let row = |record: String| {
        vec![(
            "_fleet_sync_seed_attempts",
            serde_json::json!([text(&attempt.to_string()), text(&record)]),
        )]
    };

    // Act / Assert
    passes_and_damage_is_refused(
        row(json(&SeedId { generation: 1 })),
        row("\"one\"".into()),
        "_fleet_sync_seed_attempts",
    )
    .await;
}

fn install() -> SnapshotInstall {
    SnapshotInstall {
        install_id: InstallId::new(uuid("01a1c070-0000-7000-8000-0000000c0001")).expect("install"),
        database: database(),
        position: position(3, 17),
        schema: schema(),
    }
}
fn install_marker(ready: bool) -> (&'static str, Value) {
    let source = CopyStatus {
        database: database(),
        role: CopyRole::Authoritative,
        fence: Epoch::new(3).expect("epoch"),
        holder: Some(holder()),
        start: position(1, 0),
        acked: position(3, 17),
        schema: schema(),
    };
    let marker = serde_json::json!({
        "install": install(),
        "source": source,
        "schemaSteps": null,
        "verified": null,
        "ready": ready,
    });
    (
        "_fleet_sync_install",
        serde_json::json!([integer(1), text(&marker.to_string())]),
    )
}

#[tokio::test]
async fn an_install_marker_decodes_and_a_ready_but_unverified_one_is_refused() {
    passes_and_damage_is_refused(
        vec![install_marker(false)],
        vec![install_marker(true)],
        "_fleet_sync_install",
    )
    .await;
}

#[tokio::test]
async fn an_install_page_receipt_decodes_and_one_out_of_order_is_refused() {
    // Arrange
    let receipt = |page: u32| fleet_sync_receiver::InstallPageReceipt {
        install_id: install().install_id,
        page,
        digest: PageDigest::of(&snapshot_page()).expect("page digest"),
        rows: 0,
        next: None,
    };
    let row = |page: u32| {
        vec![
            install_marker(false),
            (
                "_fleet_sync_install_pages",
                serde_json::json!([
                    text(&install().install_id.to_string()),
                    integer(0),
                    text(&json(&receipt(page)))
                ]),
            ),
        ]
    };

    // Act / Assert
    passes_and_damage_is_refused(row(0), row(1), "_fleet_sync_install_pages").await;
}

/// Part `index` of the two-part transaction committed at `commit` in epoch 3.
fn staged_part(commit: u64, index: u32) -> Piece<board_sync_protocol::BoardAnnotation> {
    let row = |identity: u64| {
        let key = SqlValue::Text(format!("01a1c070-0000-7000-8000-0000000d{identity:04x}"));
        RowChange::upsert(
            "records",
            vec![key.clone()],
            vec![key, SqlValue::Text(format!("row {identity}"))],
        )
        .expect("row change")
    };
    let whole = ReplicatedTransaction::Rows {
        commit_id: CommitSequence::new(commit),
        rows: vec![row(1), row(2)],
    };
    let key = TransactionKey {
        epoch: Epoch::new(3).expect("epoch"),
        commit_id: CommitSequence::new(commit),
    };
    Piece::new(PieceContents {
        database: database(),
        epoch: Epoch::new(3).expect("epoch"),
        continues_from: position(3, 17),
        through: position(3, commit),
        transactions: vec![ReplicatedTransaction::Rows {
            commit_id: CommitSequence::new(commit),
            rows: vec![row(u64::from(index) + 1)],
        }],
        part: Some(
            TransactionPart::new(
                key,
                index,
                2,
                PieceDigest::of_transaction(&whole).expect("digest"),
            )
            .expect("part"),
        ),
        annotation: None,
    })
    .expect("piece")
}

fn staged_row(part_index: i64, staged_at: &str, payload: &str) -> Vec<(&'static str, Value)> {
    vec![(
        "_fleet_sync_staged_parts",
        serde_json::json!([
            text(r#"{"epoch":3,"commitId":18}"#),
            integer(part_index),
            text(staged_at),
            text(payload)
        ]),
    )]
}

/// G3: a staged part main's apply would refuse (payload, metadata, index,
/// transaction or digest) is refused; so is one main's inspection marks damaged.
#[tokio::test]
async fn a_real_staged_part_passes_and_each_payload_or_time_damage_is_refused() {
    // Arrange: part 0 of transaction {epoch 3, commit 18}, as apply stores it.
    let payload = json(&staged_part(18, 0));
    let control = staged_row(0, "1794000000000", &payload);
    let digest_damaged = payload.replace("row 1", "row 9");
    assert_ne!(
        digest_damaged, payload,
        "the digest damage must change the payload"
    );
    let cases = [
        (
            "malformed payload",
            staged_row(0, "1794000000000", "{}"),
            "_fleet_sync_staged_parts",
        ),
        (
            "index mismatch",
            staged_row(1, "1794000000000", &payload),
            "_fleet_sync_staged_parts",
        ),
        (
            "transaction mismatch",
            staged_row(0, "1794000000000", &json(&staged_part(19, 0))),
            "_fleet_sync_staged_parts",
        ),
        (
            "digest mismatch",
            staged_row(0, "1794000000000", &digest_damaged),
            "_fleet_sync_staged_parts",
        ),
    ];

    // Act
    let control = run_gate(&with_rows(control)).await;
    let time_damaged = run_gate(&with_rows(staged_row(0, "not-an-i64", &payload))).await;

    // Assert
    assert_eq!(control.outcome, Outcome::Pass, "{control:?}");
    for (name, rows, table) in cases {
        let report = run_gate(&with_rows(rows)).await;
        assert_eq!(report.outcome, Outcome::Refused, "{name}: {report:?}");
        assert_refused_at(&report, table);
    }
    assert!(
        time_damaged
            .refusals
            .iter()
            .any(|refusal| refusal.object.starts_with("copy ")
                && refusal.row == "inspection"
                && refusal.reason.contains("damaged")),
        "{:?}",
        time_damaged.refusals
    );
}

#[tokio::test]
async fn a_row_no_receiver_reader_reads_is_refused_by_its_count() {
    // Arrange: a seed page with no seed record; the receiver reads pages only
    // for the generation its seed record names, so this page is never read.
    let contents = snapshot_page();
    let page = SeedPage {
        seed: SeedId { generation: 1 },
        page: 0,
        digest: PageDigest::of(&contents).expect("page digest"),
        contents,
    };
    let orphan = with_rows(vec![(
        "_fleet_sync_seed_pages",
        serde_json::json!([
            text("1"),
            integer(0),
            text(page.digest.as_str()),
            text(&json(&page))
        ]),
    )]);

    // Act
    let report = run_gate(&orphan).await;

    // Assert
    assert_eq!(report.outcome, Outcome::Refused, "{report:?}");
    assert!(
        report
            .refusals
            .iter()
            .any(|refusal| refusal.row == "_fleet_sync_seed_pages"
                && refusal.reason == "main's receiver readers decode 0 of 1 rows"),
        "{:?}",
        report.refusals
    );
}
