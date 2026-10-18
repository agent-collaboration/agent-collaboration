#![allow(clippy::panic)]
use collaboration_central_core::*;

use super::*;

fn epoch_piece(
    writer_epoch: u64,
    from: ReplicationPosition,
    through: u64,
    changes: Vec<RowChange>,
) -> Piece {
    Piece::new(PieceContents {
        project: project(),
        epoch: epoch(writer_epoch),
        continues_from: from,
        through: position(writer_epoch, through),
        transactions: vec![rows(through, changes)],
        part: None,
        summary: summary(),
    })
    .expect("epoch piece")
}

#[test]
fn old_epoch_cutoff_does_not_follow_later_replication_or_later_fences() {
    let mut store = setup();
    call(
        &mut store,
        WireCall::Copy(CopyCall::Fence {
            holder: machine(1),
            transfer_id: transfer_id(),
            new_epoch: epoch(2),
            expect: FenceExpectation {
                epoch: epoch(1),
                acked: position(1, 1),
            },
            target_schema_max: 1,
        }),
    );
    assert_eq!(
        call(
            &mut store,
            apply_call(epoch_piece(
                2,
                position(1, 1),
                5,
                vec![row(2, "new holder")]
            ))
        ),
        WireResult::Copy(CopyResult::ApplyPiece(SessionAnswer::Acked {
            through: position(2, 5)
        }))
    );
    call(
        &mut store,
        WireCall::Copy(CopyCall::Fence {
            holder: machine(1),
            transfer_id: TransferId::new("019f0000-0000-7000-8000-000000000004").expect("id"),
            new_epoch: epoch(3),
            expect: FenceExpectation {
                epoch: epoch(2),
                acked: position(2, 5),
            },
            target_schema_max: 1,
        }),
    );
    for (old_epoch, from, through, cutoff) in [
        (1, position(1, 1), 3, position(1, 1)),
        (2, position(2, 5), 7, position(2, 5)),
    ] {
        assert_eq!(
            call(
                &mut store,
                apply_call(epoch_piece(old_epoch, from, through, vec![row(9, "tail")]))
            ),
            WireResult::Copy(CopyResult::ApplyPiece(SessionAnswer::Refused(
                SessionRefusal::StaleEpoch {
                    current_epoch: epoch(3),
                    epoch_ended_at: cutoff
                }
            )))
        );
    }
    assert_eq!(store.record_ids(), vec![2]);
}

#[test]
fn not_initialized_archived_future_epoch_and_sender_digest_mismatch_are_distinct() {
    let mut empty = SqlTxFake::new();
    assert_eq!(
        call(
            &mut empty,
            apply_call(piece(1, 3, vec![rows(3, vec![row(1, "row")])]))
        ),
        WireResult::Copy(CopyResult::ApplyPiece(SessionAnswer::Refused(
            SessionRefusal::NotInitialized
        )))
    );
    let mut store = setup();
    assert_eq!(
        call(
            &mut store,
            apply_call(epoch_piece(2, position(1, 1), 3, vec![row(1, "row")]))
        ),
        WireResult::Copy(CopyResult::ApplyPiece(SessionAnswer::Refused(
            SessionRefusal::NotWriter {
                current: Some(epoch(1))
            }
        )))
    );
    let mut mismatch = piece(1, 3, vec![rows(3, vec![row(1, "row")])]);
    mismatch.digest = PieceDigest::try_from(
        "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa".to_owned(),
    )
    .expect("digest");
    assert_eq!(
        call(&mut store, apply_call(mismatch)),
        WireResult::Copy(CopyResult::ApplyPiece(SessionAnswer::Refused(
            SessionRefusal::PieceConflict
        )))
    );
    store
        .connection
        .execute("UPDATE _copy_state SET archived=1", [])
        .expect("archive");
    assert_eq!(
        call(
            &mut store,
            apply_call(piece(1, 3, vec![rows(3, vec![row(1, "row")])]))
        ),
        WireResult::Copy(CopyResult::ApplyPiece(SessionAnswer::Acked {
            through: position(1, 3)
        }))
    );
    assert_eq!(store.record_ids(), vec![1]);
}

#[test]
fn applied_part_receipts_retain_each_parts_digest_and_original_ack() {
    let mut store = setup();
    let parts = parts(2);
    for part in &parts {
        call(&mut store, apply_call(part.clone()));
    }
    call(
        &mut store,
        apply_call(piece(4, 7, vec![rows(7, vec![row(9, "later")])])),
    );
    for part in &parts {
        assert_eq!(
            call(&mut store, apply_call(part.clone())),
            WireResult::Copy(CopyResult::ApplyPiece(SessionAnswer::Acked {
                through: position(1, 4)
            }))
        );
        let mut changed = part.clone();
        changed.transactions = vec![rows(4, vec![row(99, "conflict")])];
        changed.digest = changed.recomputed_digest().expect("digest");
        assert_eq!(
            call(&mut store, apply_call(changed)),
            WireResult::Copy(CopyResult::ApplyPiece(SessionAnswer::Refused(
                SessionRefusal::PieceConflict
            )))
        );
    }
    assert_eq!(store.record_ids(), vec![0, 1, 9]);
}

#[test]
fn count_and_whole_digest_disagreement_cannot_combine_fragments() {
    let mut store = setup();
    let first = parts(3).first().expect("part").clone();
    call(&mut store, apply_call(first));
    let last = parts(2).last().expect("part").clone();
    assert_eq!(
        call(&mut store, apply_call(last)),
        WireResult::Copy(CopyResult::ApplyPiece(SessionAnswer::Refused(
            SessionRefusal::PositionGap {
                acked: position(1, 1)
            }
        )))
    );
    assert_eq!(
        store
            .connection
            .query_row("SELECT count(*) FROM _copy_staged_parts", [], |row| row
                .get::<_, i64>(0))
            .expect("count"),
        0
    );
    assert!(store.record_ids().is_empty());
}

#[test]
fn snapshot_pages_are_read_only_position_labelled_and_restart_after_copy_moves() {
    let mut store = setup();
    call(
        &mut store,
        apply_call(piece(
            1,
            3,
            vec![rows(3, (0..300).map(|id| row(id, "record")).collect())],
        )),
    );
    let page = match call(
        &mut store,
        WireCall::Copy(CopyCall::SnapshotPage { cursor: None }),
    ) {
        WireResult::Copy(CopyResult::SnapshotPage(SessionAnswer::ReadPage(
            ReadPage::ProjectCopy(ProjectCopyRead::Page(page)),
        ))) => page,
        result => panic!("expected snapshot page, received {result:?}"),
    };
    assert_eq!(page.project, project());
    assert_eq!(page.position, position(1, 3));
    assert_eq!(page.schema.version, 1);
    assert_eq!(page.rows.len(), 256);
    assert_eq!(store.record_ids().len(), 300);
    let cursor = page.next.expect("next page");
    let continuation = match call(
        &mut store,
        WireCall::Copy(CopyCall::SnapshotPage {
            cursor: Some(cursor.clone()),
        }),
    ) {
        WireResult::Copy(CopyResult::SnapshotPage(SessionAnswer::ReadPage(
            ReadPage::ProjectCopy(ProjectCopyRead::Page(page)),
        ))) => page,
        result => panic!("expected continuation page, received {result:?}"),
    };
    assert_eq!(continuation.rows.len(), 44);
    assert_eq!(continuation.position, position(1, 3));
    assert!(continuation.next.is_none());
    call(
        &mut store,
        apply_call(piece(3, 7, vec![rows(7, vec![row(400, "new")])])),
    );
    assert_eq!(
        call(
            &mut store,
            WireCall::Copy(CopyCall::SnapshotPage {
                cursor: Some(cursor)
            })
        ),
        WireResult::Copy(CopyResult::SnapshotPage(SessionAnswer::ReadPage(
            ReadPage::ProjectCopy(ProjectCopyRead::Moved {
                position: position(1, 7)
            })
        )))
    );
}

#[test]
fn schema_and_rows_roll_back_together_on_a_late_constraint_error() {
    let mut store = setup();
    let request = piece(
        1,
        5,
        vec![
            ReplicatedTransaction::SchemaStep {
                commit_id: CommitId::new(3),
                migration_version: 2,
                statements: vec!["CREATE TABLE more_records(id INTEGER PRIMARY KEY)".into()],
            },
            rows(
                5,
                vec![
                    RowChange::upsert(
                        "records",
                        vec![SqlValue::Integer(1)],
                        vec![SqlValue::Integer(1), SqlValue::Null],
                    )
                    .expect("row"),
                ],
            ),
        ],
    );
    assert!(matches!(
        store.run(input(apply_call(request)), now()),
        Err(CentralError::Store(StoreError::Constraint { .. }))
    ));
    assert_eq!(
        store
            .connection
            .query_row(
                "SELECT count(*) FROM sqlite_schema WHERE name='more_records'",
                [],
                |row| row.get::<_, i64>(0)
            )
            .expect("table count"),
        0
    );
    assert_eq!(ack(&mut store), position(1, 1));
    assert!(store.record_ids().is_empty());
}

#[test]
fn engine_and_replication_bookkeeping_do_not_change_schema_identity() {
    let mut store = setup();
    let before = match call(
        &mut store,
        WireCall::Copy(CopyCall::CopyStatus { project: project() }),
    ) {
        WireResult::Copy(CopyResult::CopyStatus(Some(status))) => status.schema,
        result => panic!("expected status, received {result:?}"),
    };
    store.connection.execute_batch("CREATE TABLE turso_cdc(id INTEGER PRIMARY KEY); CREATE TABLE _replication_state(id INTEGER PRIMARY KEY)").expect("internal tables");
    let request = piece(
        1,
        3,
        vec![ReplicatedTransaction::SchemaStep {
            commit_id: CommitId::new(3),
            migration_version: 2,
            statements: vec!["CREATE TABLE turso_metadata(id INTEGER PRIMARY KEY)".into()],
        }],
    );
    call(&mut store, apply_call(request));
    let after = match call(
        &mut store,
        WireCall::Copy(CopyCall::CopyStatus { project: project() }),
    ) {
        WireResult::Copy(CopyResult::CopyStatus(Some(status))) => status.schema,
        result => panic!("expected status, received {result:?}"),
    };
    assert_eq!(after.fingerprint, before.fingerprint);
}

#[test]
fn platform_tables_are_absent_from_snapshots_and_cannot_receive_replicated_rows() {
    for name in [
        "__MINIFLARE_do_name",
        "_CF_KV",
        "TuRsO_metadata",
        "__TURSO_INTERNAL_seq",
        "_REPLICATION_state",
        "_SQLX_MIGRATIONS",
    ] {
        let mut store = setup();
        store.connection.execute_batch(&format!("CREATE TABLE \"{name}\"(id INTEGER PRIMARY KEY, label TEXT); INSERT INTO \"{name}\" VALUES(1, 'private')")).expect("platform fixture");
        let page = call(
            &mut store,
            WireCall::Copy(CopyCall::SnapshotPage { cursor: None }),
        );
        let WireResult::Copy(CopyResult::SnapshotPage(SessionAnswer::ReadPage(
            ReadPage::ProjectCopy(ProjectCopyRead::Page(page)),
        ))) = page
        else {
            panic!("snapshot page");
        };
        assert_eq!(page.table, "records", "{name}");
        assert!(page.rows.is_empty(), "{name}");
        assert!(page.next.is_none(), "{name}");
        let change = RowChange::upsert(
            name,
            vec![SqlValue::Integer(1)],
            vec![SqlValue::Integer(1), SqlValue::Text("forged".into())],
        )
        .expect("row shape");
        assert!(
            matches!(
                store.run(
                    input(apply_call(piece(1, 2, vec![rows(2, vec![change])]))),
                    now()
                ),
                Err(CentralError::InvalidInput { .. })
            ),
            "{name}"
        );
        assert_eq!(ack(&mut store), position(1, 1));
        assert_eq!(
            store
                .connection
                .query_row(
                    &format!("SELECT label FROM \"{name}\" WHERE id=1"),
                    [],
                    |row| row.get::<_, String>(0)
                )
                .expect("private row"),
            "private"
        );
    }
}
