use super::super::engine_evidence::{
    assert_fixture_attempt_ids_are_uuid_v7, witness_fixture_attempt_ids,
};
use super::super::{Statement, support::*};
use central_storage::SqlValue;
use collaboration_central_core::engine_probe::InstanceKey;
use collaboration_central_core::engine_probe::{
    EngineReceipt, EngineRequestOutcome, ProbeEvent, ProbeRequest, ProbeState, SeedProbe,
};
use std::sync::atomic::Ordering;

#[tokio::test]
async fn probe_requests_claim_one_receipt_and_replay_the_original_typed_answer() {
    let fixture = Fixture::new().await;
    witness_fixture_attempt_ids(&fixture).await;
    let runtime = fixture.host.central(&fixture.caller.organization_id).await;
    let item = uuid(8600);
    let group = uuid(8601);
    runtime
        .seed_probe(SeedProbe {
            key: item,
            group,
            due: None,
            claim: "one effect".into(),
        })
        .await
        .unwrap();
    let request = ProbeRequest {
        request_id: id(8602),
        key: InstanceKey(item.to_string()),
        event: ProbeEvent::Run,
    };

    let answer = runtime
        .request_probe(&fixture.caller, request.clone())
        .await
        .unwrap();
    let replay = runtime
        .request_probe(&fixture.caller, request.clone())
        .await
        .unwrap();

    assert_eq!(answer, replay);
    assert_eq!(
        answer,
        EngineRequestOutcome::Answer {
            receipt: EngineReceipt::Applied {
                state: ProbeState::Fired
            }
        }
    );
    let reused = runtime
        .request_probe(
            &fixture.caller,
            ProbeRequest {
                event: ProbeEvent::Stop,
                ..request
            },
        )
        .await
        .unwrap();
    assert_eq!(
        reused,
        EngineRequestOutcome::Answer {
            receipt: EngineReceipt::RequestIdReused
        }
    );
    let rows=runtime.statement_host().read(Statement {
        sql:"SELECT effect_count,(SELECT COUNT(*) FROM runtime_request_log WHERE request_id=?) FROM engine_probe_groups WHERE group_id=?",
        args:vec![text(id::<workspace_model::RequestId>(8602)),text(group)],
    }).await.unwrap();
    assert_eq!(
        rows.rows,
        vec![vec![
            SqlValue::Integer(1.try_into().unwrap()),
            SqlValue::Integer(1.try_into().unwrap())
        ]]
    );
    assert_eq!(
        runtime
            .control::<Option<i64>>("raw-alarm-time", &())
            .await
            .unwrap(),
        None,
        "traffic-driven work never arms the alarm"
    );
    assert_fixture_attempt_ids_are_uuid_v7(
        &fixture,
        "probe_requests_claim_one_receipt_and_replay_the_original_typed_answer",
        &[],
    )
    .await;
    fixture.host.close().await;
}

#[tokio::test]
async fn probe_receipt_claim_replaces_only_an_expired_row_at_the_exact_millisecond_boundary() {
    let mut fixture = Fixture::new().await;
    witness_fixture_attempt_ids(&fixture).await;
    let runtime = fixture.host.central(&fixture.caller.organization_id).await;
    let start =
        chrono::DateTime::from_timestamp_millis(fixture.clock.0.load(Ordering::SeqCst)).unwrap();
    let group = uuid(8610);
    for number in [8611, 8612, 8613] {
        runtime
            .seed_probe(SeedProbe {
                key: uuid(number),
                group,
                due: None,
                claim: format!("claim{number}"),
            })
            .await
            .unwrap();
    }
    let first = ProbeRequest {
        request_id: id(8614),
        key: InstanceKey(uuid(8611).to_string()),
        event: ProbeEvent::Run,
    };
    let original = runtime
        .request_probe(&fixture.caller, first.clone())
        .await
        .unwrap();
    fixture.clock.0.store(
        (start + chrono::Duration::hours(24)).timestamp_millis(),
        Ordering::SeqCst,
    );
    fixture.caller = super::super::support::fresh_auth(&fixture.host, &fixture.caller);
    assert_eq!(
        runtime.request_probe(&fixture.caller, first).await.unwrap(),
        original
    );
    let reused = runtime
        .request_probe(
            &fixture.caller,
            ProbeRequest {
                request_id: id(8614),
                key: InstanceKey(uuid(8612).to_string()),
                event: ProbeEvent::Run,
            },
        )
        .await
        .unwrap();
    assert_eq!(
        reused,
        EngineRequestOutcome::Answer {
            receipt: EngineReceipt::RequestIdReused
        }
    );
    fixture.clock.0.store(
        (start + chrono::Duration::hours(24) + chrono::Duration::milliseconds(1))
            .timestamp_millis(),
        Ordering::SeqCst,
    );
    fixture.caller = super::super::support::fresh_auth(&fixture.host, &fixture.caller);

    let answer = runtime
        .request_probe(
            &fixture.caller,
            ProbeRequest {
                request_id: id(8614),
                key: InstanceKey(uuid(8613).to_string()),
                event: ProbeEvent::Run,
            },
        )
        .await
        .unwrap();

    assert_eq!(
        answer,
        EngineRequestOutcome::Answer {
            receipt: EngineReceipt::Applied {
                state: ProbeState::Fired
            }
        }
    );
    let rows=runtime.statement_host().read(Statement {
        sql:"SELECT effect_count,(SELECT state FROM engine_probe_items WHERE item_id=?),(SELECT COUNT(*) FROM runtime_request_log WHERE request_id=?) FROM engine_probe_groups WHERE group_id=?",
        args:vec![text(uuid(8612)),text(id::<workspace_model::RequestId>(8614)),text(group)],
    }).await.unwrap();
    assert_eq!(
        rows.rows,
        vec![vec![
            SqlValue::Integer(2.try_into().unwrap()),
            text("waiting"),
            SqlValue::Integer(1.try_into().unwrap())
        ]]
    );
    assert_fixture_attempt_ids_are_uuid_v7(
        &fixture,
        "probe_receipt_claim_replaces_only_an_expired_row_at_the_exact_millisecond_boundary",
        &[],
    )
    .await;
    fixture.host.close().await;
}

fn uuid(number: u32) -> uuid::Uuid {
    format!("01890f2e-7b4c-7cc0-98c4-{number:012}")
        .parse()
        .unwrap()
}

#[tokio::test]
async fn probe_commit_answer_loss_never_charges_a_failure_or_duplicates_an_effect() {
    use collaboration_central_core::{EngineCommitFault, EngineCommitFaultKind};
    let fixture = Fixture::new().await;
    witness_fixture_attempt_ids(&fixture).await;
    let runtime = fixture.host.central(&fixture.caller.organization_id).await;
    for (number, kind, applied) in [
        (8620, EngineCommitFaultKind::AppliedAnswerLost, true),
        (8623, EngineCommitFaultKind::NotAppliedAnswerLost, false),
    ] {
        let item = uuid(number);
        let group = uuid(number + 1);
        runtime
            .seed_probe(SeedProbe {
                key: item,
                group,
                due: None,
                claim: format!("uncertain{number}"),
            })
            .await
            .unwrap();
        runtime
            .inject_engine_commit_fault(EngineCommitFault {
                key: InstanceKey(item.to_string()),
                kind,
            })
            .await;
        let request = ProbeRequest {
            request_id: id(number + 2),
            key: InstanceKey(item.to_string()),
            event: ProbeEvent::Run,
        };

        assert_eq!(
            runtime
                .request_probe(&fixture.caller, request.clone())
                .await
                .unwrap(),
            EngineRequestOutcome::OutcomeUnknown
        );

        let rows=runtime.statement_host().read(Statement {
            sql:"SELECT effect_count,(SELECT COUNT(*) FROM engine_work_attempts WHERE subject_key=?),(SELECT COUNT(*) FROM runtime_request_log WHERE request_id=?) FROM engine_probe_groups WHERE group_id=?",
            args:vec![text(item),text(id::<workspace_model::RequestId>(number+2)),text(group)],
        }).await.unwrap();
        assert_eq!(
            rows.rows,
            vec![vec![
                SqlValue::Integer(i64::from(applied).try_into().unwrap()),
                SqlValue::Integer(0.try_into().unwrap()),
                SqlValue::Integer(i64::from(applied).try_into().unwrap())
            ]]
        );
        assert_eq!(
            runtime
                .request_probe(&fixture.caller, request)
                .await
                .unwrap(),
            EngineRequestOutcome::Answer {
                receipt: EngineReceipt::Applied {
                    state: ProbeState::Fired
                }
            }
        );
        let rows = runtime
            .statement_host()
            .read(Statement {
                sql: "SELECT effect_count FROM engine_probe_groups WHERE group_id=?",
                args: vec![text(group)],
            })
            .await
            .unwrap();
        assert_eq!(
            rows.rows,
            vec![vec![SqlValue::Integer(1.try_into().unwrap())]]
        );
    }
    assert_fixture_attempt_ids_are_uuid_v7(
        &fixture,
        "probe_commit_answer_loss_never_charges_a_failure_or_duplicates_an_effect",
        &[],
    )
    .await;
    fixture.host.close().await;
}

#[tokio::test]
async fn a_constraint_rejected_request_rolls_back_only_its_instance_and_answers_without_a_record() {
    let fixture = Fixture::new().await;
    witness_fixture_attempt_ids(&fixture).await;
    let runtime = fixture.host.central(&fixture.caller.organization_id).await;
    let group = uuid(8630);
    for number in [8631, 8632, 8633] {
        runtime
            .seed_probe(SeedProbe {
                key: uuid(number),
                group,
                due: None,
                claim: if number == 8633 {
                    "independent".into()
                } else {
                    "same unique claim".into()
                },
            })
            .await
            .unwrap();
    }
    let request = |number| ProbeRequest {
        request_id: id(number + 10),
        key: InstanceKey(uuid(number).to_string()),
        event: ProbeEvent::Run,
    };
    assert!(matches!(
        runtime
            .request_probe(&fixture.caller, request(8631))
            .await
            .unwrap(),
        EngineRequestOutcome::Answer {
            receipt: EngineReceipt::Applied { .. }
        }
    ));

    assert_eq!(
        runtime
            .request_probe(&fixture.caller, request(8632))
            .await
            .unwrap(),
        EngineRequestOutcome::ItemFailed {
            failure:
                collaboration_central_core::engine_probe::ItemFailureClass::ConstraintViolated {
                    constraint: central_storage::ConstraintKind::Unique
                }
        }
    );

    assert!(matches!(
        runtime
            .request_probe(&fixture.caller, request(8633))
            .await
            .unwrap(),
        EngineRequestOutcome::Answer {
            receipt: EngineReceipt::Applied { .. }
        }
    ));
    let rows=runtime.statement_host().read(Statement {
        sql:"SELECT state,(SELECT effect_count FROM engine_probe_groups WHERE group_id=?),((SELECT COUNT(*) FROM engine_work_attempts WHERE subject_key=?)),(SELECT COUNT(*) FROM runtime_request_log WHERE request_id=?) FROM engine_probe_items WHERE item_id=?",
        args:vec![text(group),text(uuid(8632)),text(id::<workspace_model::RequestId>(8642)),text(uuid(8632))],
    }).await.unwrap();
    assert_eq!(
        rows.rows,
        vec![vec![
            text("waiting"),
            SqlValue::Integer(2.try_into().unwrap()),
            SqlValue::Integer(0.try_into().unwrap()),
            SqlValue::Integer(0.try_into().unwrap())
        ]]
    );
    assert_fixture_attempt_ids_are_uuid_v7(
        &fixture,
        "a_constraint_rejected_request_rolls_back_only_its_instance_and_answers_without_a_record",
        &[],
    )
    .await;
    fixture.host.close().await;
}
