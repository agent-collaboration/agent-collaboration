#![allow(clippy::expect_used)]

#[path = "copy_cases/extra_cases.rs"]
mod extra_cases;
mod support;

use chrono::{DateTime, TimeDelta, Utc};
use collaboration_central_core::*;
use collaboration_replication_protocol::*;
use support::SqlTxFake;

fn machine(n: u64) -> InstallationId {
    InstallationId::new(format!("019f0000-0000-7000-8000-{n:012x}")).expect("machine")
}
fn apply_call(piece: Piece) -> WireCall {
    WireCall::Copy(CopyCall::ApplyPiece {
        piece,
        sender: machine(1),
    })
}
fn project() -> ProjectId {
    ProjectId::new("019f0000-0000-7000-8000-000000000001").expect("id")
}
fn epoch(value: u64) -> Epoch {
    Epoch::new(value).expect("epoch")
}
fn position(epoch_value: u64, commit: u64) -> ReplicationPosition {
    ReplicationPosition::new(epoch(epoch_value), CommitId::new(commit))
}
fn call_id() -> ObjectCallId {
    ObjectCallId::new("019f0000-0000-7000-8000-000000000002").expect("id")
}
fn transfer_id() -> TransferId {
    TransferId::new("019f0000-0000-7000-8000-000000000003").expect("id")
}
fn now() -> RegistryTime {
    RegistryTime(
        DateTime::parse_from_rfc3339("2026-10-12T12:00:00Z")
            .expect("time")
            .with_timezone(&Utc),
    )
}
fn input(call: WireCall) -> WireInput {
    WireInput::Call {
        from: ObjectAddress::Registry,
        call_id: call_id(),
        call: Box::new(call),
    }
}
fn call(store: &mut SqlTxFake, request: WireCall) -> WireResult {
    let input = input(request);
    let effects = store.run(input.clone(), now()).expect("step");
    validate_call_returns(&input, &effects).expect("exactly one matching Return");
    effects
        .into_iter()
        .find_map(|effect| match effect {
            WireEffect::Return { result, .. } => Some(result),
            _ => None,
        })
        .expect("Return")
}
fn summary() -> Option<ProjectSummary> {
    Some(ProjectSummary {
        status: "open".into(),
        latest_record_at: now().0,
    })
}
fn piece(from: u64, through: u64, transactions: Vec<ReplicatedTransaction>) -> Piece {
    Piece::new(PieceContents {
        project: project(),
        epoch: epoch(1),
        continues_from: position(1, from),
        through: position(1, through),
        transactions,
        part: None,
        summary: summary(),
    })
    .expect("piece")
}
fn row(id: i64, text: &str) -> RowChange {
    RowChange::upsert(
        "records",
        vec![SqlValue::Integer(id)],
        vec![SqlValue::Integer(id), SqlValue::Text(text.into())],
    )
    .expect("row")
}
fn rows(commit_id: u64, row_changes: Vec<RowChange>) -> ReplicatedTransaction {
    ReplicatedTransaction::Rows {
        commit_id: CommitId::new(commit_id),
        rows: row_changes,
    }
}
fn setup() -> SqlTxFake {
    let mut store = SqlTxFake::new();
    assert!(matches!(
        call(
            &mut store,
            WireCall::Copy(CopyCall::Initialize {
                holder: machine(1),
                project: project(),
                epoch: epoch(1)
            })
        ),
        WireResult::Copy(CopyResult::Initialized(_))
    ));
    let schema = piece(
        0,
        1,
        vec![ReplicatedTransaction::SchemaStep {
            commit_id: CommitId::new(1),
            migration_version: 1,
            statements: vec![
                "CREATE TABLE records(id INTEGER PRIMARY KEY, label TEXT NOT NULL)".into(),
            ],
        }],
    );
    assert_eq!(
        call(&mut store, apply_call(schema)),
        WireResult::Copy(CopyResult::ApplyPiece(SessionAnswer::Acked {
            through: position(1, 1)
        }))
    );
    store
}
fn ack(store: &mut SqlTxFake) -> ReplicationPosition {
    match call(
        store,
        WireCall::Copy(CopyCall::CopyStatus { project: project() }),
    ) {
        WireResult::Copy(CopyResult::CopyStatus(Some(status))) => status.acked,
        result => {
            assert!(matches!(
                result,
                WireResult::Copy(CopyResult::CopyStatus(Some(_)))
            ));
            position(1, 0)
        }
    }
}
fn parts(count: u32) -> Vec<Piece> {
    let transaction = rows(
        4,
        (0..count)
            .map(|index| row(i64::from(index), "part"))
            .collect(),
    );
    let digest = PieceDigest::of_transaction(&transaction).expect("digest");
    (0..count)
        .map(|index| {
            Piece::new(PieceContents {
                project: project(),
                epoch: epoch(1),
                continues_from: position(1, 1),
                through: position(1, 4),
                transactions: vec![rows(4, vec![row(i64::from(index), "part")])],
                part: Some(
                    TransactionPart::new(
                        TransactionKey {
                            epoch: epoch(1),
                            commit_id: CommitId::new(4),
                        },
                        index,
                        count,
                        digest.clone(),
                    )
                    .expect("part"),
                ),
                summary: (index == count - 1).then(|| summary().expect("summary")),
            })
            .expect("piece")
        })
        .collect()
}

#[test]
fn incoming_call_produces_exactly_one_return() {
    let mut store = SqlTxFake::new();
    let input = input(WireCall::Copy(CopyCall::Initialize {
        holder: machine(1),
        project: project(),
        epoch: epoch(1),
    }));
    let effects = store.run(input.clone(), now()).expect("step");
    assert_eq!(validate_call_returns(&input, &effects), Ok(()));
}

#[test]
fn call_conformance_rejects_missing_duplicate_wrong_id_and_wrong_kind_returns() {
    let input = input(WireCall::Copy(CopyCall::CopyStatus { project: project() }));
    assert_eq!(
        validate_call_returns(&input, &[]),
        Err(CentralError::ReplyContract)
    );
    let returned = WireEffect::Return {
        call_id: call_id(),
        result: WireResult::Copy(CopyResult::CopyStatus(None)),
    };
    assert_eq!(
        validate_call_returns(&input, &[returned.clone(), returned.clone()]),
        Err(CentralError::ReplyContract)
    );
    let wrong_id = WireEffect::Return {
        call_id: ObjectCallId::new("019f0000-0000-7000-8000-000000000009").expect("id"),
        result: WireResult::Copy(CopyResult::CopyStatus(None)),
    };
    assert_eq!(
        validate_call_returns(&input, &[wrong_id]),
        Err(CentralError::ReplyContract)
    );
    let wrong_kind = WireEffect::Return {
        call_id: call_id(),
        result: WireResult::Copy(CopyResult::ApplyPiece(SessionAnswer::TailStored)),
    };
    assert_eq!(
        validate_call_returns(&input, &[wrong_kind]),
        Err(CentralError::ReplyContract)
    );
    assert_eq!(validate_call_returns(&input, &[returned]), Ok(()));
}

#[test]
fn ordered_apply_uses_declared_keys_and_preserves_commit_order() {
    let mut store = setup();
    let request = piece(
        1,
        7,
        vec![
            rows(3, vec![row(9, "first")]),
            rows(
                7,
                vec![
                    row(9, "updated"),
                    row(10, "second"),
                    RowChange::delete("records", vec![SqlValue::Integer(10)]).expect("delete"),
                ],
            ),
        ],
    );
    assert_eq!(
        call(&mut store, apply_call(request)),
        WireResult::Copy(CopyResult::ApplyPiece(SessionAnswer::Acked {
            through: position(1, 7)
        }))
    );
    assert_eq!(store.record_ids(), vec![9]);
    assert_eq!(
        store
            .connection
            .query_row("SELECT label FROM records WHERE id=9", [], |row| row
                .get::<_, String>(0))
            .expect("label"),
        "updated"
    );
}

#[test]
fn repeat_returns_its_original_ack_after_later_pieces_and_changed_digest_is_refused() {
    let mut store = setup();
    let first = piece(1, 3, vec![rows(3, vec![row(1, "first")])]);
    let expected = call(&mut store, apply_call(first.clone()));
    call(
        &mut store,
        apply_call(piece(3, 7, vec![rows(7, vec![row(2, "later")])])),
    );
    assert_eq!(call(&mut store, apply_call(first)), expected);
    let changed = piece(1, 3, vec![rows(3, vec![row(1, "different")])]);
    assert_eq!(
        call(&mut store, apply_call(changed)),
        WireResult::Copy(CopyResult::ApplyPiece(SessionAnswer::Refused(
            SessionRefusal::PieceConflict
        )))
    );
    assert_eq!(ack(&mut store), position(1, 7));
    assert_eq!(store.record_ids(), vec![1, 2]);
}

#[test]
fn missing_receipt_is_never_blindly_acknowledged_and_gap_names_current_position() {
    let mut store = setup();
    let gap = piece(3, 4, vec![rows(4, vec![row(1, "gap")])]);
    assert_eq!(
        call(&mut store, apply_call(gap)),
        WireResult::Copy(CopyResult::ApplyPiece(SessionAnswer::Refused(
            SessionRefusal::PositionGap {
                acked: position(1, 1)
            }
        )))
    );
    store
        .connection
        .execute("DELETE FROM _copy_receipts", [])
        .expect("delete receipt");
    let old = piece(0, 1, vec![rows(1, vec![row(1, "old")])]);
    assert_eq!(
        call(&mut store, apply_call(old)),
        WireResult::Copy(CopyResult::ApplyPiece(SessionAnswer::Refused(
            SessionRefusal::PositionGap {
                acked: position(1, 1)
            }
        )))
    );
    assert!(store.record_ids().is_empty());
}

#[test]
fn fence_survives_memory_loss_and_stale_epoch_uses_its_original_cutoff() {
    let mut store = setup();
    let request = WireCall::Copy(CopyCall::Fence {
        holder: machine(1),
        transfer_id: transfer_id(),
        new_epoch: epoch(2),
        expect: FenceExpectation {
            epoch: epoch(1),
            acked: position(1, 1),
        },
        target_schema_max: 1,
    });
    let result = call(&mut store, request.clone());
    assert_eq!(
        result,
        WireResult::Copy(CopyResult::Fence(FenceResult::Fenced {
            old_epoch: epoch(1),
            new_epoch: epoch(2),
            start: position(1, 1)
        }))
    );
    assert_eq!(call(&mut store, request), result);
    assert_eq!(
        call(
            &mut store,
            apply_call(piece(1, 3, vec![rows(3, vec![row(1, "tail")])]))
        ),
        WireResult::Copy(CopyResult::ApplyPiece(SessionAnswer::Refused(
            SessionRefusal::StaleEpoch {
                current_epoch: epoch(2),
                epoch_ended_at: position(1, 1)
            }
        )))
    );
    assert!(store.record_ids().is_empty());
}

#[test]
fn fence_checks_preview_position_schema_and_forward_motion_in_the_transaction() {
    let mut store = setup();
    for (expect, new_epoch, target_schema_max, expected) in [
        (
            FenceExpectation {
                epoch: epoch(1),
                acked: position(1, 0),
            },
            epoch(2),
            1,
            FenceResult::PreviewStale {
                acked: position(1, 1),
            },
        ),
        (
            FenceExpectation {
                epoch: epoch(1),
                acked: position(1, 1),
            },
            epoch(1),
            1,
            FenceResult::FenceNotAdvanced {
                current_epoch: epoch(1),
            },
        ),
    ] {
        assert_eq!(
            call(
                &mut store,
                WireCall::Copy(CopyCall::Fence {
                    holder: machine(1),
                    transfer_id: transfer_id(),
                    new_epoch,
                    expect,
                    target_schema_max
                })
            ),
            WireResult::Copy(CopyResult::Fence(expected))
        );
    }
    assert!(matches!(
        call(
            &mut store,
            WireCall::Copy(CopyCall::Fence {
                holder: machine(1),
                transfer_id: transfer_id(),
                new_epoch: epoch(2),
                expect: FenceExpectation {
                    epoch: epoch(1),
                    acked: position(1, 1)
                },
                target_schema_max: 0
            })
        ),
        WireResult::Copy(CopyResult::Fence(FenceResult::SchemaTooNew {
            supported: 0,
            ..
        }))
    ));
}

#[test]
fn two_part_transaction_is_invisible_until_complete_and_each_part_replays_after_commit() {
    let mut store = setup();
    let parts = parts(2);
    let first = parts.first().expect("first").clone();
    let last = parts.last().expect("last").clone();
    assert_eq!(
        call(&mut store, apply_call(first.clone())),
        WireResult::Copy(CopyResult::ApplyPiece(SessionAnswer::PartStaged {
            transaction: TransactionKey {
                epoch: epoch(1),
                commit_id: CommitId::new(4)
            },
            index: 0,
            acked: position(1, 1)
        }))
    );
    assert!(store.record_ids().is_empty());
    assert_eq!(ack(&mut store), position(1, 1));
    let expected = WireResult::Copy(CopyResult::ApplyPiece(SessionAnswer::Acked {
        through: position(1, 4),
    }));
    assert_eq!(call(&mut store, apply_call(last.clone())), expected);
    assert_eq!(store.record_ids(), vec![0, 1]);
    assert_eq!(call(&mut store, apply_call(first)), expected);
    assert_eq!(call(&mut store, apply_call(last)), expected);
}

#[test]
fn repeated_staged_part_matches_piece_digest_and_changed_content_conflicts() {
    let mut store = setup();
    let first = parts(2).first().expect("first").clone();
    let expected = call(&mut store, apply_call(first.clone()));
    assert_eq!(call(&mut store, apply_call(first.clone())), expected);
    let mut changed = first;
    changed.transactions = vec![rows(4, vec![row(0, "changed")])];
    changed.digest = changed.recomputed_digest().expect("digest");
    assert_eq!(
        call(&mut store, apply_call(changed)),
        WireResult::Copy(CopyResult::ApplyPiece(SessionAnswer::Refused(
            SessionRefusal::PieceConflict
        )))
    );
    assert!(store.record_ids().is_empty());
}

#[test]
fn missing_middle_and_cleanup_before_final_never_apply_a_short_transaction() {
    for cleanup in [false, true] {
        let mut store = setup();
        let parts = parts(3);
        call(
            &mut store,
            apply_call(parts.first().expect("first").clone()),
        );
        if cleanup {
            store
                .run(
                    WireInput::Alarm {
                        face: CallerKind::Copy,
                    },
                    RegistryTime(now().0 + TimeDelta::days(2)),
                )
                .expect("cleanup");
        }
        assert_eq!(
            call(&mut store, apply_call(parts.last().expect("last").clone())),
            WireResult::Copy(CopyResult::ApplyPiece(SessionAnswer::Refused(
                SessionRefusal::PositionGap {
                    acked: position(1, 1)
                }
            )))
        );
        assert!(store.record_ids().is_empty());
        assert_eq!(
            store
                .connection
                .query_row("SELECT count(*) FROM _copy_staged_parts", [], |row| row
                    .get::<_, i64>(0))
                .expect("count"),
            0
        );
    }
}

#[test]
fn whole_transaction_digest_mismatch_drops_staging_without_applying() {
    let mut store = setup();
    let mut parts = parts(2);
    let wrong = PieceDigest::try_from(
        "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa".to_owned(),
    )
    .expect("digest");
    for part in &mut parts {
        part.part.as_mut().expect("part").transaction_digest = wrong.clone();
        part.digest = part.recomputed_digest().expect("digest");
    }
    call(
        &mut store,
        apply_call(parts.first().expect("first").clone()),
    );
    assert_eq!(
        call(&mut store, apply_call(parts.last().expect("last").clone())),
        WireResult::Copy(CopyResult::ApplyPiece(SessionAnswer::Refused(
            SessionRefusal::PositionGap {
                acked: position(1, 1)
            }
        )))
    );
    assert!(store.record_ids().is_empty());
}

#[test]
fn copy_full_rolls_back_all_rows_and_ack_before_returning_typed_refusal() {
    let mut store = setup();
    store.fail_on_record_insert = Some(2);
    let request = input(apply_call(piece(
        1,
        3,
        vec![rows(3, vec![row(1, "first"), row(2, "full")])],
    )));
    let error = store.run(request.clone(), now()).expect_err("full");
    let effects = error.rollback_trace();
    validate_call_returns(&request, &effects).expect("typed Return after rollback");
    assert_eq!(
        effects,
        vec![WireEffect::Return {
            call_id: call_id(),
            result: WireResult::Copy(CopyResult::ApplyPiece(SessionAnswer::Refused(
                SessionRefusal::CentralCopyFull
            )))
        }]
    );
    assert!(store.record_ids().is_empty());
    assert_eq!(ack(&mut store), position(1, 1));
}

#[test]
fn corrupt_stored_state_fails_closed_instead_of_defaulting() {
    let mut store = setup();
    store
        .connection
        .execute("UPDATE _copy_state SET fence_epoch='invalid'", [])
        .expect("corrupt");
    assert!(matches!(
        store.run(
            input(WireCall::Copy(CopyCall::CopyStatus { project: project() })),
            now()
        ),
        Err(CentralError::Store(StoreError::Corrupt { .. }))
    ));
}
