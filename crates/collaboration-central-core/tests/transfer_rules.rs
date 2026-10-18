#![allow(clippy::expect_used, clippy::panic)]
mod registry_support;
use collaboration_central_core::*;
use collaboration_replication_protocol::*;
use registry_support::*;
fn move_call(t: u64) -> WireCall {
    admin(AdminRequest::Move {
        transfer_id: transfer(t),
        project: project(),
        to_installation: machine(2),
        reason: "move".into(),
    })
}
#[test]
fn piece_between_confirmation_and_fence_refuses_without_grant() {
    let mut h = Harness::setup();
    let call = h.takeover_call(1);
    let (input, effects) = h.begin("registry", call);
    h.schema_piece(1);
    assert_eq!(
        h.finish("registry", &input, effects).0,
        WireResult::Registry(RegistryResult::Admin(AdminResult::Refused(
            AdminRefusal::PreviewStale {
                acked: position(1, 1)
            }
        )))
    );
    assert!(matches!(h.renew(1,1,1),SessionAnswer::Renewed{current,..} if current.epoch==epoch(1)));
}
#[test]
fn schema_support_checked_before_fence_and_unknown_target_refused() {
    let mut h = Harness::setup();
    h.schema_piece(1);
    h.register(2, 0);
    let call = h.takeover_call(1);
    assert!(matches!(
        h.call("registry", call),
        WireResult::Registry(RegistryResult::Admin(AdminResult::Refused(
            AdminRefusal::SchemaTooNew {
                supported: Some(0),
                ..
            }
        )))
    ));
    let mut h = Harness::setup();
    let p = h.preview();
    assert!(matches!(
        h.call(
            "registry",
            admin(AdminRequest::Takeover {
                transfer_id: transfer(2),
                project: project(),
                to_installation: machine(3),
                reason: "unknown".into(),
                confirm_token: p.confirm_token
            })
        ),
        WireResult::Registry(RegistryResult::Admin(AdminResult::Refused(
            AdminRefusal::SchemaTooNew {
                supported: None,
                ..
            }
        )))
    ));
}
#[test]
fn interrupted_fence_repeated_request_completes_same_epoch_and_conflicting_request_refused() {
    let mut h = Harness::setup();
    let call = h.takeover_call(1);
    let (_, effects) = h.begin("registry", call.clone());
    let dispatch = effects
        .into_iter()
        .find_map(|e| match e {
            WireEffect::Dispatch { call, .. } => Some(call),
            _ => None,
        })
        .expect("fence");
    assert!(
        matches!(h.call("copy",dispatch),WireResult::Copy(CopyResult::Fence(FenceResult::Fenced{new_epoch,..})) if new_epoch==epoch(2))
    );
    assert!(
        matches!(h.call("registry",call.clone()),WireResult::Registry(RegistryResult::Admin(AdminResult::Granted{new_epoch,..})) if new_epoch==epoch(2))
    );
    assert!(
        matches!(h.call("registry",call),WireResult::Registry(RegistryResult::Admin(AdminResult::Granted{new_epoch,..})) if new_epoch==epoch(2))
    );
    assert_eq!(
        h.call("registry", move_call(1)),
        WireResult::Registry(RegistryResult::Admin(AdminResult::Refused(
            AdminRefusal::ConflictingRequest
        )))
    );
}
#[test]
fn move_pending_one_transfer_only_and_renewal_recovers_missed_release_notice() {
    let mut h = Harness::setup();
    assert_eq!(
        h.call("registry", move_call(1)),
        WireResult::Registry(RegistryResult::Admin(AdminResult::Pending {
            transfer_id: transfer(1)
        }))
    );
    assert_eq!(
        h.call("registry", move_call(2)),
        WireResult::Registry(RegistryResult::Admin(AdminResult::Refused(
            AdminRefusal::TransferInProgress {
                transfer_id: transfer(1)
            }
        )))
    );
    assert!(
        matches!(h.renew(1,1,1),SessionAnswer::Renewed{transfer_in_progress:Some(t),..} if t==transfer(1))
    );
    assert_eq!(
        h.call(
            "registry",
            WireCall::Registry(RegistryCall::Release {
                installation: machine(1),
                project: project(),
                epoch: epoch(1),
                transfer: transfer(1),
                outcome: ReleaseOutcome::Final(position(1, 0))
            })
        ),
        WireResult::Registry(RegistryResult::Release(SessionAnswer::Released {
            final_position: position(1, 0)
        }))
    );
    assert!(
        matches!(h.call("registry",move_call(1)),WireResult::Registry(RegistryResult::Admin(AdminResult::Granted{new_epoch,..})) if new_epoch==epoch(2))
    );
}
#[test]
fn migration_pending_or_failed_refusal_is_durable_and_changes_no_lease() {
    for state in [MigrationState::Pending, MigrationState::Failed] {
        let mut h = Harness::setup();
        h.call("registry", move_call(1));
        let reason = ReleaseRefusal::MigrationPending {
            migration: 2,
            state,
        };
        let release = WireCall::Registry(RegistryCall::Release {
            installation: machine(1),
            project: project(),
            epoch: epoch(1),
            transfer: transfer(1),
            outcome: ReleaseOutcome::Refused(HolderReleaseRefusal::MigrationPending {
                migration: 2,
                state,
            }),
        });
        let expected =
            WireResult::Registry(RegistryResult::Release(SessionAnswer::ReleaseRefused {
                transfer: transfer(1),
                reason: reason.clone(),
            }));
        assert_eq!(h.call("registry", release.clone()), expected);
        assert_eq!(h.call("registry", release), expected);
        assert_eq!(
            h.call("registry", move_call(1)),
            WireResult::Registry(RegistryResult::Admin(AdminResult::Refused(
                AdminRefusal::MigrationPending(reason)
            )))
        );
        assert!(
            matches!(h.renew(1,1,1),SessionAnswer::Renewed{current,transfer_in_progress:None,..} if current.epoch==epoch(1))
        );
    }
}
#[test]
fn move_refuses_disconnected_holder_or_thirty_minute_boundary() {
    for minute in [29, 30, 31] {
        let mut h = Harness::setup();
        h.now = at(minute);
        let result = h.call("registry", move_call(1));
        assert_eq!(
            matches!(
                result,
                WireResult::Registry(RegistryResult::Admin(AdminResult::Pending { .. }))
            ),
            minute < 30
        );
    }
    let mut h = Harness::setup();
    h.call(
        "registry",
        WireCall::Registry(RegistryCall::SessionDisconnected {
            installation: machine(1),
            connection: connection(1),
        }),
    );
    assert!(matches!(
        h.call("registry", move_call(1)),
        WireResult::Registry(RegistryResult::Admin(AdminResult::Refused(
            AdminRefusal::HolderUnreachable { .. }
        )))
    ));
}
#[test]
fn restored_registry_never_issues_epoch_at_or_below_copy_fence() {
    let mut h = Harness::setup();
    h.call(
        "copy",
        WireCall::Copy(CopyCall::Fence {
            holder: machine(1),
            transfer_id: transfer(9),
            new_epoch: epoch(5),
            expect: FenceExpectation {
                epoch: epoch(1),
                acked: position(1, 0),
            },
            target_schema_max: 10,
        }),
    );
    let call = h.takeover_call(1);
    assert!(
        matches!(h.call("registry",call),WireResult::Registry(RegistryResult::Admin(AdminResult::Granted{new_epoch,..})) if new_epoch==epoch(6))
    );
}

#[test]
fn another_installation_cannot_write_known_current_epoch() {
    let mut h = Harness::setup();
    let p = Piece::new(PieceContents {
        project: project(),
        epoch: epoch(1),
        continues_from: position(1, 0),
        through: position(1, 1),
        transactions: vec![ReplicatedTransaction::Rows {
            commit_id: 1.into(),
            rows: vec![],
        }],
        part: None,
        summary: Some(ProjectSummary {
            status: "active".into(),
            latest_record_at: h.now.0,
        }),
    })
    .expect("piece");
    assert_eq!(
        h.call(
            "copy",
            WireCall::Copy(CopyCall::ApplyPiece {
                piece: p,
                sender: machine(2)
            })
        ),
        WireResult::Copy(CopyResult::ApplyPiece(SessionAnswer::Refused(
            SessionRefusal::NotWriter {
                current: Some(epoch(1))
            }
        )))
    );
}

#[test]
fn release_gap_can_be_retried_after_the_final_piece_reaches_copy() {
    let mut h = Harness::setup();
    h.call("registry", move_call(1));
    let release = WireCall::Registry(RegistryCall::Release {
        installation: machine(1),
        project: project(),
        epoch: epoch(1),
        transfer: transfer(1),
        outcome: ReleaseOutcome::Final(position(1, 1)),
    });
    assert_eq!(
        h.call("registry", release.clone()),
        WireResult::Registry(RegistryResult::Release(SessionAnswer::Refused(
            SessionRefusal::PositionGap {
                acked: position(1, 0)
            }
        )))
    );
    h.schema_piece(1);
    assert_eq!(
        h.call("registry", release),
        WireResult::Registry(RegistryResult::Release(SessionAnswer::Released {
            final_position: position(1, 1)
        }))
    );
}
#[test]
fn schema_refusal_at_release_is_typed_replayed_and_never_grants() {
    let mut h = Harness::setup();
    h.schema_piece(1);
    h.register(2, 0);
    h.call("registry", move_call(1));
    let release = WireCall::Registry(RegistryCall::Release {
        installation: machine(1),
        project: project(),
        epoch: epoch(1),
        transfer: transfer(1),
        outcome: ReleaseOutcome::Final(position(1, 1)),
    });
    let answer = h.call("registry", release.clone());
    assert!(
        matches!(&answer,WireResult::Registry(RegistryResult::Release(SessionAnswer::ReleaseRefused{reason:ReleaseRefusal::SchemaTooNew{store,supported:Some(0)},..})) if store.version==1)
    );
    assert_eq!(h.call("registry", release), answer);
    assert!(matches!(
        h.call("registry", move_call(1)),
        WireResult::Registry(RegistryResult::Admin(AdminResult::Refused(
            AdminRefusal::SchemaTooNew { .. }
        )))
    ));
    assert!(matches!(h.renew(1,1,1),SessionAnswer::Renewed{current,..} if current.epoch==epoch(1)));
}
#[test]
fn historical_holder_gets_immutable_stale_cutoff_other_sender_gets_not_writer() {
    let mut h = Harness::setup();
    h.schema_piece(1);
    let call = h.takeover_call(1);
    h.call("registry", call);
    let piece = Piece::new(PieceContents {
        project: project(),
        epoch: epoch(1),
        continues_from: position(1, 1),
        through: position(1, 2),
        transactions: vec![ReplicatedTransaction::Rows {
            commit_id: 2.into(),
            rows: vec![],
        }],
        part: None,
        summary: Some(ProjectSummary {
            status: "active".into(),
            latest_record_at: h.now.0,
        }),
    })
    .expect("piece");
    assert_eq!(
        h.call(
            "copy",
            WireCall::Copy(CopyCall::ApplyPiece {
                piece: piece.clone(),
                sender: machine(1)
            })
        ),
        WireResult::Copy(CopyResult::ApplyPiece(SessionAnswer::Refused(
            SessionRefusal::StaleEpoch {
                current_epoch: epoch(2),
                epoch_ended_at: position(1, 1)
            }
        )))
    );
    assert_eq!(
        h.call(
            "copy",
            WireCall::Copy(CopyCall::ApplyPiece {
                piece,
                sender: machine(3)
            })
        ),
        WireResult::Copy(CopyResult::ApplyPiece(SessionAnswer::Refused(
            SessionRefusal::NotWriter {
                current: Some(epoch(2))
            }
        )))
    );
}
#[test]
fn no_epoch_records_refuses_preview_without_grant_or_floor_write() {
    let mut h = Harness::setup();
    h.store("registry")
        .connection
        .execute("DELETE FROM _registry_epoch_floor", [])
        .expect("remove floor");
    h.store("copy")
        .connection
        .execute("DELETE FROM _copy_state", [])
        .expect("remove copy evidence");
    assert_eq!(
        h.call(
            "registry",
            admin(AdminRequest::TakeoverPreview { project: project() })
        ),
        WireResult::Registry(RegistryResult::Admin(AdminResult::Refused(
            AdminRefusal::RegistryEpochUnproven
        )))
    );
    let count: i64 = h
        .store("registry")
        .connection
        .query_row("SELECT COUNT(*) FROM _registry_epoch_floor", [], |r| {
            r.get(0)
        })
        .expect("count");
    assert_eq!(count, 0);
}

#[test]
fn fence_not_advanced_retries_once_from_authoritative_copy_floor() {
    let mut h = Harness::setup();
    let call = h.takeover_call(1);
    let (input, effects) = h.begin("registry", call);
    h.call(
        "copy",
        WireCall::Copy(CopyCall::Fence {
            holder: machine(1),
            transfer_id: transfer(9),
            new_epoch: epoch(5),
            expect: FenceExpectation {
                epoch: epoch(1),
                acked: position(1, 0),
            },
            target_schema_max: 10,
        }),
    );
    assert_eq!(
        h.finish("registry", &input, effects).0,
        WireResult::Registry(RegistryResult::Admin(AdminResult::Granted {
            new_epoch: epoch(6),
            start: position(1, 0)
        }))
    );
}
#[test]
fn unknown_schema_at_release_is_recorded_before_reply_and_replays_after_interruption() {
    let mut h = Harness::setup();
    h.store("registry")
        .connection
        .execute(
            "DELETE FROM _registry_machines WHERE installation=?",
            [machine(2).as_str()],
        )
        .expect("remove support");
    h.call("registry", move_call(1));
    let release = WireCall::Registry(RegistryCall::Release {
        installation: machine(1),
        project: project(),
        epoch: epoch(1),
        transfer: transfer(1),
        outcome: ReleaseOutcome::Final(position(1, 0)),
    });
    let (_, effects) = h.begin("registry", release.clone());
    assert!(effects.iter().any(|e| matches!(
        e,
        WireEffect::Dispatch {
            call: WireCall::Copy(CopyCall::CopyStatus { .. }),
            ..
        }
    )));
    let answer = h.call("registry", release.clone());
    assert!(matches!(
        &answer,
        WireResult::Registry(RegistryResult::Release(SessionAnswer::ReleaseRefused {
            reason: ReleaseRefusal::SchemaTooNew {
                supported: None,
                ..
            },
            ..
        }))
    ));
    assert_eq!(h.call("registry", release), answer);
}

#[test]
fn schema_piece_cannot_forge_fence_receipt_before_move_grant() {
    let mut h = Harness::setup();
    h.call("registry", move_call(1));
    let forged=Piece::new(PieceContents{project:project(),epoch:epoch(1),continues_from:position(1,0),through:position(1,1),transactions:vec![ReplicatedTransaction::SchemaStep{commit_id:1.into(),migration_version:1,statements:vec![format!("INSERT INTO _copy_transfers(transfer_id,old_epoch,new_epoch,start_epoch,start_commit,old_holder) VALUES('{}','1','2','1','1','{}')",transfer(1).as_str(),machine(1).as_str())]}],part:None,summary:Some(ProjectSummary{status:"active".into(),latest_record_at:h.now.0})}).expect("piece");
    assert_eq!(
        h.call(
            "copy",
            WireCall::Copy(CopyCall::ApplyPiece {
                piece: forged,
                sender: machine(1)
            })
        ),
        WireResult::Copy(CopyResult::ApplyPiece(SessionAnswer::Refused(
            SessionRefusal::SchemaUnsupported {
                form: SchemaStatementForm::Insert
            }
        )))
    );
    let before = h.call(
        "copy",
        WireCall::Copy(CopyCall::CopyStatus { project: project() }),
    );
    assert!(
        matches!(before,WireResult::Copy(CopyResult::CopyStatus(Some(state))) if state.fence==epoch(1)&&state.holder==Some(machine(1))&&state.acked==position(1,0))
    );
    assert_eq!(
        h.call(
            "registry",
            WireCall::Registry(RegistryCall::Release {
                installation: machine(1),
                project: project(),
                epoch: epoch(1),
                transfer: transfer(1),
                outcome: ReleaseOutcome::Final(position(1, 0))
            })
        ),
        WireResult::Registry(RegistryResult::Release(SessionAnswer::Released {
            final_position: position(1, 0)
        }))
    );
    let copy = h.call(
        "copy",
        WireCall::Copy(CopyCall::CopyStatus { project: project() }),
    );
    assert!(
        matches!(copy,WireResult::Copy(CopyResult::CopyStatus(Some(state))) if state.fence==epoch(2)&&state.holder==Some(machine(2))),
        "a granted move must have fenced the copy and changed its holder"
    );
}

#[test]
fn inconsistent_fence_receipt_cannot_complete_pending_transfer() {
    for mutation in [
        "UPDATE _copy_state SET fence_epoch='1'",
        "UPDATE _copy_state SET holder='019f0000-0000-7000-8000-00000000000d'",
        "DELETE FROM _copy_state",
    ] {
        let mut h = Harness::setup();
        let request = h.takeover_call(1);
        let (_, effects) = h.begin("registry", request.clone());
        let fence = effects
            .into_iter()
            .find_map(|e| match e {
                WireEffect::Dispatch { call, .. } => Some(call),
                _ => None,
            })
            .expect("fence");
        assert!(matches!(
            h.call("copy", fence.clone()),
            WireResult::Copy(CopyResult::Fence(FenceResult::Fenced { .. }))
        ));
        h.store("copy")
            .connection
            .execute(mutation, [])
            .expect("corrupt state");
        let input = h.input(fence);
        let time = h.now;
        assert!(
            matches!(
                h.store("copy").run(input, time),
                Err(CentralError::Store(StoreError::Corrupt { .. }))
            ),
            "{mutation}"
        );
        let stored: String = h
            .store("registry")
            .connection
            .query_row("SELECT record FROM _registry_transfers", [], |r| r.get(0))
            .expect("transfer");
        let transfer: TransferRecord = serde_json::from_str(&stored).expect("record");
        assert!(matches!(transfer.state, TransferState::Fencing { .. }));
        assert!(
            matches!(h.renew(1,5,1),SessionAnswer::Renewed{current,..}if current.epoch==epoch(1)&&current.holder==machine(1))
        );
    }
}
#[test]
fn whole_piece_schema_refusal_applies_no_earlier_rows_or_schema() {
    let mut h = Harness::setup();
    let piece = Piece::new(PieceContents {
        project: project(),
        epoch: epoch(1),
        continues_from: position(1, 0),
        through: position(1, 2),
        transactions: vec![
            ReplicatedTransaction::SchemaStep {
                commit_id: 1.into(),
                migration_version: 1,
                statements: vec!["CREATE TABLE permitted(id INTEGER PRIMARY KEY)".into()],
            },
            ReplicatedTransaction::SchemaStep {
                commit_id: 2.into(),
                migration_version: 2,
                statements: vec!["PRAGMA user_version=99".into()],
            },
        ],
        part: None,
        summary: Some(ProjectSummary {
            status: "active".into(),
            latest_record_at: h.now.0,
        }),
    })
    .expect("piece");
    assert_eq!(
        h.call("copy", apply_call(piece)),
        WireResult::Copy(CopyResult::ApplyPiece(SessionAnswer::Refused(
            SessionRefusal::SchemaUnsupported {
                form: SchemaStatementForm::Pragma
            }
        )))
    );
    let tables: i64 = h
        .store("copy")
        .connection
        .query_row(
            "SELECT COUNT(*) FROM sqlite_schema WHERE name='permitted'",
            [],
            |r| r.get(0),
        )
        .expect("tables");
    assert_eq!(tables, 0);
    assert!(
        matches!(h.call("copy",WireCall::Copy(CopyCall::CopyStatus{project:project()})),WireResult::Copy(CopyResult::CopyStatus(Some(state)))if state.acked==position(1,0)&&state.schema.version==0)
    );
}
#[test]
fn mixed_case_reserved_row_table_is_rejected_without_mutation() {
    let mut h = Harness::setup();
    let piece = Piece::new(PieceContents {
        project: project(),
        epoch: epoch(1),
        continues_from: position(1, 0),
        through: position(1, 1),
        transactions: vec![ReplicatedTransaction::Rows {
            commit_id: 1.into(),
            rows: vec![RowChange::Delete {
                table: "_CoPy_State".into(),
                key: vec![SqlValue::Integer(1)],
            }],
        }],
        part: None,
        summary: Some(ProjectSummary {
            status: "active".into(),
            latest_record_at: h.now.0,
        }),
    })
    .expect("piece");
    let input = h.input(apply_call(piece));
    let time = h.now;
    assert!(matches!(
        h.store("copy").run(input, time),
        Err(CentralError::InvalidInput { .. })
    ));
    assert!(
        matches!(h.call("copy",WireCall::Copy(CopyCall::CopyStatus{project:project()})),WireResult::Copy(CopyResult::CopyStatus(Some(state))) if state.acked==position(1,0))
    );
}
