#![allow(clippy::expect_used)]

use crate::test_fixtures::*;
use crate::*;
use serde_json::json;

#[test]
fn notice_positions_are_positive_and_fleet_hints_keep_a_separate_cursor() {
    assert!(NoticePosition::new(0).is_err());
    assert!(serde_json::from_value::<NoticePosition>(json!(0)).is_err());
    assert!(serde_json::from_value::<ReadCursor>(json!({"kind":"notices","value":0})).is_err());
    let notice: Notice = serde_json::from_value(
        json!({"position":1,"kind":{"kind":"fleetFeedAdvanced","through":900}}),
    )
    .expect("numbered hint");
    assert_eq!(notice.position.value(), 1);
    assert_eq!(
        notice.kind,
        NoticeKind::FleetFeedAdvanced {
            through: FleetPosition::new(900)
        }
    );
}

#[test]
fn every_machine_and_central_frame_variant_round_trips() {
    assert_variant(
        MachineFrame::Hello {
            installation: installation_id(),
            session_id: session_id(),
            schema_support: SchemaSupport { max_version: 7 },
        },
        "hello",
    );
    for (request, tag) in request_samples() {
        assert_variant(request.clone(), tag);
        assert_variant(
            MachineFrame::Request {
                frame_id: frame_id(),
                body: request,
            },
            "request",
        );
    }
    assert_variant(
        CentralFrame::Welcome {
            session_id: session_id(),
            projects: vec![ProjectSessionState {
                project: project_id(),
                acked: position(1, 2),
                lease: lease_status(),
            }],
        },
        "welcome",
    );
    let mut answers = vec![
        (
            SessionAnswer::Acked {
                through: position(1, 2),
            },
            "acked",
        ),
        (
            SessionAnswer::PartStaged {
                transaction: TransactionKey {
                    epoch: epoch(1),
                    commit_id: CommitId::new(2),
                },
                index: 0,
                acked: position(1, 1),
            },
            "partStaged",
        ),
        (
            SessionAnswer::Renewed {
                expires_at: timestamp(),
                transfer_in_progress: Some(transfer_id()),
                replayed: false,
                current: lease_status(),
            },
            "renewed",
        ),
        (
            SessionAnswer::Released {
                final_position: position(1, 2),
            },
            "released",
        ),
        (
            SessionAnswer::FirstLease {
                epoch: epoch(1),
                expires_at: timestamp(),
            },
            "firstLease",
        ),
        (SessionAnswer::TailStored, "tailStored"),
        (
            SessionAnswer::FleetDecision {
                request_id: request_id(),
                decision: FleetPayload::new(json!({"kind": "accepted"})),
            },
            "fleetDecision",
        ),
    ];
    answers.extend(
        read_page_samples()
            .into_iter()
            .map(|(page, _)| (SessionAnswer::ReadPage(page), "readPage")),
    );
    answers.extend(
        refusal_samples()
            .into_iter()
            .map(|(refusal, _)| (SessionAnswer::Refused(refusal), "refused")),
    );
    for (answer, tag) in answers {
        assert_variant(answer.clone(), tag);
        assert_variant(
            CentralFrame::Answer {
                frame_id: frame_id(),
                body: answer,
            },
            "answer",
        );
    }
    for (kind, tag) in notice_samples() {
        assert_variant(kind.clone(), tag);
        assert_variant(
            CentralFrame::Notice {
                position: NoticePosition::new(1).expect("notice position"),
                kind,
            },
            "notice",
        );
    }
    for (rejection, tag) in [
        (CredentialRejection::Unknown, "unknown"),
        (CredentialRejection::Expired, "expired"),
        (CredentialRejection::Revoked, "revoked"),
        (CredentialRejection::NotMachine, "notMachine"),
    ] {
        assert_variant(rejection.clone(), tag);
        let reason = SessionCloseReason::CredentialRejected(rejection);
        assert_variant(reason.clone(), "credentialRejected");
        assert_variant(CentralFrame::Closed { reason }, "closed");
    }
    assert_variant(SessionCloseReason::ConnectionReplaced, "connectionReplaced");
    assert_variant(
        CentralFrame::Closed {
            reason: SessionCloseReason::ConnectionReplaced,
        },
        "closed",
    );
}

#[test]
fn every_read_page_and_refusal_variant_round_trips() {
    for (page, tag) in read_page_samples() {
        assert_variant(page, tag);
    }
    for (refusal, tag) in refusal_samples() {
        assert_variant(refusal, tag);
    }
    assert_variant(ReadCursor::Fleet(FleetPosition::new(2)), "fleet");
    assert_variant(
        ReadCursor::Notices(NoticePosition::new(2).expect("notice position")),
        "notices",
    );
    assert_variant(ReadCursor::Snapshot(snapshot_cursor()), "snapshot");
    assert_variant(ProjectCopyRead::Page(Box::new(snapshot_page())), "page");
    assert_variant(
        ProjectCopyRead::Moved {
            position: position(2, 3),
        },
        "moved",
    );
    for (scope, tag) in [
        (ReadScope::FleetRecords, "fleetRecords"),
        (
            ReadScope::Directory {
                query: FleetPayload::new(json!({})),
            },
            "directory",
        ),
        (
            ReadScope::ProjectCopy {
                project: project_id(),
            },
            "projectCopy",
        ),
        (ReadScope::Notices, "notices"),
        (
            ReadScope::TailReports {
                project: project_id(),
            },
            "tailReports",
        ),
    ] {
        assert_variant(scope, tag);
    }
}

#[test]
fn every_lease_transfer_and_archive_payload_variant_round_trips() {
    assert_variant(
        MoveResult::Moved {
            new_epoch: epoch(2),
            start: position(1, 2),
        },
        "moved",
    );
    for (reason, tag) in [
        (MoveRefusalReason::HolderUnreachable, "holderUnreachable"),
        (MoveRefusalReason::MigrationPending, "migrationPending"),
        (MoveRefusalReason::TransferInProgress, "transferInProgress"),
    ] {
        assert_variant(reason.clone(), tag);
        assert_variant(MoveResult::Refused { reason }, "refused");
    }
    let preview = TakeoverPreview {
        holder: installation_id(),
        epoch: epoch(1),
        last_seen: timestamp(),
        acked: position(1, 2),
        tail_estimate_ms: 500,
        confirm_token: "token".into(),
    };
    assert_eq!(
        serde_json::from_value::<TakeoverPreview>(serde_json::to_value(&preview).expect("encode"))
            .expect("decode"),
        preview
    );
    assert_variant(
        TakeoverResult::Granted {
            new_epoch: epoch(2),
            start: position(1, 2),
        },
        "granted",
    );
    assert_variant(
        TakeoverResult::Pending {
            transfer_id: transfer_id(),
        },
        "pending",
    );
    for (reason, tag) in [
        (TakeoverRefusalReason::PreviewStale, "previewStale"),
        (
            TakeoverRefusalReason::TransferInProgress,
            "transferInProgress",
        ),
    ] {
        assert_variant(reason.clone(), tag);
        assert_variant(TakeoverResult::Refused { reason }, "refused");
    }
    let seen = ArchiveSeen {
        board: board_id(),
        project: project_id(),
        position: position(1, 2),
    };
    assert_eq!(
        serde_json::from_value::<ArchiveSeen>(serde_json::to_value(&seen).expect("encode"))
            .expect("decode"),
        seen
    );
}

#[test]
fn frame_wire_tags_are_camel_case_and_hello_has_no_credential() {
    let hello = serde_json::to_value(MachineFrame::Hello {
        installation: installation_id(),
        session_id: session_id(),
        schema_support: SchemaSupport { max_version: 7 },
    })
    .expect("encode");
    assert_eq!(
        hello,
        json!({"kind": "hello", "installation": installation_id(), "session_id": session_id(), "schema_support": {"maxVersion": 7}})
    );
    assert!(hello.get("credential").is_none());
}

#[test]
fn invalid_tags_and_ids_are_rejected() {
    for value in [
        json!(""),
        json!("not-a-uuid"),
        json!("019f0000-0000-4000-8000-000000000001"),
        json!("019F0000-0000-7000-8000-000000000001"),
    ] {
        assert!(serde_json::from_value::<ProjectId>(value.clone()).is_err());
        assert!(serde_json::from_value::<RequestId>(value.clone()).is_err());
        assert!(serde_json::from_value::<FrameId>(value.clone()).is_err());
        assert!(serde_json::from_value::<BoardId>(value.clone()).is_err());
        assert!(serde_json::from_value::<InstallationId>(value.clone()).is_err());
        assert!(serde_json::from_value::<SessionId>(value.clone()).is_err());
        assert!(serde_json::from_value::<TransferId>(value).is_err());
    }
    assert!(serde_json::from_value::<SessionRefusal>(json!({"kind": "notARefusal"})).is_err());
    assert!(serde_json::from_value::<MachineFrame>(json!({"kind": "unknown"})).is_err());
    assert!(serde_json::from_value::<CentralFrame>(json!({"kind": "unknown"})).is_err());
    assert!(serde_json::from_value::<SessionRequest>(json!({"kind": "unknown"})).is_err());
    assert!(serde_json::from_value::<SessionAnswer>(json!({"kind": "unknown"})).is_err());
}

#[test]
fn renewal_attempt_round_trips_and_is_required() {
    assert_variant(
        SessionRequest::Renew {
            attempt: request_id(),
            project: project_id(),
            epoch: epoch(1),
        },
        "renew",
    );
    for attempt in [None, Some(json!("invalid"))] {
        let mut fields = serde_json::Map::new();
        fields.insert("project".into(), json!(project_id()));
        fields.insert("epoch".into(), json!(1));
        if let Some(attempt) = attempt {
            fields.insert("attempt".into(), attempt);
        }
        assert!(
            serde_json::from_value::<SessionRequest>(json!({"kind": "renew", "value": fields}))
                .is_err()
        );
    }
}

#[test]
fn renewed_answer_round_trips_fresh_and_replayed_current_lease() {
    for replayed in [false, true] {
        let mut current = lease_status();
        if replayed {
            current.epoch = epoch(2);
            current.state = LeaseState::Expired;
            current.remaining_ms = 0;
        }
        assert_variant(
            SessionAnswer::Renewed {
                expires_at: timestamp(),
                transfer_in_progress: None,
                replayed,
                current,
            },
            "renewed",
        );
    }
}

#[test]
fn datetime_uses_rfc3339_on_the_wire() {
    let encoded = serde_json::to_value(lease_status()).expect("encode");
    assert_eq!(
        encoded.get("expiresAtRegistry"),
        Some(&json!("2026-10-11T12:34:56Z"))
    );
}

#[test]
fn fleet_payload_is_the_only_json_envelope() {
    let payload = FleetPayload::new(json!({"kind": "directory"}));
    let envelope = FleetRequestEnvelope {
        request_id: request_id(),
        operation: payload.clone(),
    };
    assert_eq!(
        serde_json::from_value::<FleetRequestEnvelope>(
            serde_json::to_value(&envelope).expect("encode")
        )
        .expect("decode"),
        envelope
    );
    assert_eq!(payload.as_value(), &json!({"kind": "directory"}));
}

#[test]
fn invalid_replication_values_remain_rejected() {
    assert!(Epoch::new(0).is_err());
    let transaction = TransactionKey {
        epoch: epoch(1),
        commit_id: CommitId::new(2),
    };
    let digest = sample_piece().digest;
    assert!(TransactionPart::new(transaction, 2, 2, digest.clone()).is_err());
    assert!(TransactionPart::new(transaction, 0, 0, digest).is_err());
    let mut wire = serde_json::to_value(sample_piece()).expect("encode");
    wire.as_object_mut()
        .expect("piece object")
        .insert("through".into(), json!(position(1, 1)));
    assert!(serde_json::from_value::<Piece>(wire).is_err());
    assert!(serde_json::to_value(SqlValue::Real(f64::NAN)).is_err());
    assert!(SchemaIdentity::new(1, String::new()).is_err());
}

#[test]
fn every_replication_content_variant_round_trips_with_its_wire_tag() {
    for (value, tag) in [
        (SqlValue::Null, "null"),
        (SqlValue::Integer(i64::MIN), "integer"),
        (SqlValue::Real(1.25), "real"),
        (SqlValue::Text("text".into()), "text"),
        (SqlValue::Blob(vec![0, 1, 2, 255]), "blob"),
    ] {
        assert_variant(value, tag);
    }
    let upsert = RowChange::upsert(
        "records",
        vec![SqlValue::Integer(1)],
        vec![SqlValue::Integer(1)],
    )
    .expect("row");
    let delete = RowChange::delete("records", vec![SqlValue::Integer(1)]).expect("row");
    assert_variant(upsert.clone(), "upsert");
    assert_variant(delete.clone(), "delete");
    assert_variant(
        ReplicatedTransaction::Rows {
            commit_id: CommitId::new(2),
            rows: vec![upsert, delete],
        },
        "rows",
    );
    assert_variant(
        ReplicatedTransaction::SchemaStep {
            commit_id: CommitId::new(3),
            migration_version: 1,
            statements: vec!["CREATE TABLE records(id INTEGER PRIMARY KEY)".into()],
        },
        "schemaStep",
    );
    assert!(serde_json::from_value::<SqlValue>(json!({"kind": "unknown"})).is_err());
    assert!(serde_json::from_value::<RowChange>(json!({"kind": "unknown"})).is_err());
    assert!(serde_json::from_value::<ReplicatedTransaction>(json!({"kind": "unknown"})).is_err());
    assert!(serde_json::from_value::<ReadPage>(json!({"kind": "unknown"})).is_err());
    assert!(serde_json::from_value::<ReadScope>(json!({"kind": "unknown"})).is_err());
    assert!(serde_json::from_value::<ReadCursor>(json!({"kind": "unknown"})).is_err());
    assert!(serde_json::from_value::<ProjectCopyRead>(json!({"kind": "unknown"})).is_err());
    assert!(serde_json::from_value::<NoticeKind>(json!({"kind": "unknown"})).is_err());
    assert!(serde_json::from_value::<CredentialRejection>(json!({"kind": "unknownTag"})).is_err());
    assert!(serde_json::from_value::<SessionCloseReason>(json!({"kind": "unknown"})).is_err());
}
