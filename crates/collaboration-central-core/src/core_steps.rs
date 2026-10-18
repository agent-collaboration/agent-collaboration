//! Each face's transaction entrypoint returns only that face's legal effects.
use crate::registry_contract::PendingContinuation;
use crate::*;

pub fn step_registry(
    tx: &mut dyn SqlTx,
    input: RegistryInput,
    now: RegistryTime,
    mut ids: IdBatch,
) -> Result<Vec<RegistryEffect>, CentralError> {
    match input {
        RegistryInput::Call { call_id, call, .. } => {
            crate::registry_dispatch::call(tx, &call_id, &call, now, &mut ids)
        }
        RegistryInput::CallResult {
            call_id, result, ..
        } => {
            match result.as_ref() {
                RegistryReply::Fleet(FleetResult::LeaseIncarnationAdvanced(incarnation)) => {
                    return crate::lease_incarnation::acquired(
                        tx,
                        &call_id,
                        *incarnation,
                        now,
                        &mut ids,
                    );
                }
                RegistryReply::Fleet(FleetResult::LeaseUpdated(result)) => {
                    return crate::lease_publication::result(tx, &call_id, result, now, &mut ids);
                }
                _ => {}
            }
            if !crate::registry_dispatch::has_table(tx, "_central_continuations")? {
                return Ok(Vec::new());
            }
            match crate::registry_storage::take_continuation(tx, &call_id)? {
                Some(pending) => {
                    crate::registry_dispatch::continuation(tx, pending, *result, now, &mut ids)
                }
                None => Ok(Vec::new()),
            }
        }
        RegistryInput::Alarm => {
            let mut effects = crate::lease_publication::drive(tx, now, &mut ids)?;
            effects.extend(crate::claim_cleanup::alarm(tx, now)?);
            let earliest = effects
                .iter()
                .filter_map(|effect| match effect {
                    RegistryEffect::SetAlarm { at } => Some(*at),
                    _ => None,
                })
                .min_by_key(|at| at.0);
            effects.retain(|effect| !matches!(effect, RegistryEffect::SetAlarm { .. }));
            if let Some(at) = earliest {
                effects.insert(0, RegistryEffect::SetAlarm { at });
            }
            Ok(effects)
        }
        RegistryInput::CallFailed { .. } => Err(CentralError::ReplyContract),
    }
}
pub fn step_copy(
    tx: &mut dyn SqlTx,
    input: CopyInput,
    now: RegistryTime,
    mut ids: IdBatch,
) -> Result<Vec<CopyEffect>, CentralError> {
    if !matches!(&input, CopyInput::Health { .. })
        && let Some(retired) = crate::copy_retirement::read_retirement(tx)?
    {
        return crate::copy_retirement::answer_retired(input, retired);
    }
    let migration = if matches!(&input, CopyInput::Health { .. }) {
        None
    } else {
        Some(migrate_copy_bookkeeping(tx, now).map_err(|failure| {
            CentralError::CopyMigrationPending {
                reply: match &input {
                    CopyInput::Call { call_id, call, .. }
                        if matches!(call.as_ref(), CopyCall::Ops(CopyOpsCall::Migrate {})) =>
                    {
                        CopyMigrationReply::Migrate(call_id.clone())
                    }
                    CopyInput::Call { call_id, .. } => CopyMigrationReply::Call(call_id.clone()),
                    _ => CopyMigrationReply::NoReply,
                },
                failure,
            }
        })?)
    };
    match input {
        CopyInput::Health {
            call_id,
            storage_bytes,
        } => Ok(vec![CopyEffect::Return {
            call_id,
            result: CopyResult::Health(Box::new(crate::copy_operations::read_copy_health(
                tx,
                storage_bytes,
            ))),
        }]),
        CopyInput::Call { call_id, call, .. } => {
            let mut effects = Vec::new();
            let result = match *call {
                CopyCall::Ops(CopyOpsCall::Health {}) => {
                    return Err(CentralError::InvalidInput {
                        message: "Health requires the target shell's measurement".into(),
                    });
                }
                CopyCall::Ops(CopyOpsCall::Migrate {}) => {
                    CopyResult::Migrated(migration.ok_or(CentralError::ReplyContract)?)
                }
                CopyCall::Ops(CopyOpsCall::Repair {}) => {
                    effects = copy_alarm_duties(tx, now)?;
                    CopyResult::Repaired
                }
                CopyCall::ImportSeed { project, upload } => {
                    let (result,following)=crate::import_rules::seed(tx,&call_id,&project,&upload,now,&mut ids).map_err(|error|match error {
                        CentralError::Store(StoreError::Full)=>CentralError::ImportRejected{call_id:call_id.clone(),reason:collaboration_replication_protocol::ProjectImportRefusal::CentralCopyFull},
                        CentralError::Store(StoreError::Constraint{..})=>CentralError::ImportRejected{call_id:call_id.clone(),reason:collaboration_replication_protocol::ProjectImportRefusal::InvalidPage{field:"snapshot".into()}},
                        error=>error,
                    })?;
                    effects = following;
                    CopyResult::ImportSeed(result)
                }
                CopyCall::Initialize {
                    project,
                    epoch,
                    holder,
                } => CopyResult::Initialized(crate::copy_rules::initialize(
                    tx, &project, epoch, &holder,
                )?),
                CopyCall::CopyStatus { project } => {
                    let state = crate::copy_storage::load_copy(tx)?;
                    if state.as_ref().is_some_and(|state| state.project != project) {
                        return Err(CentralError::InvalidInput {
                            message: "copy belongs to a different project".into(),
                        });
                    }
                    CopyResult::CopyStatus(state)
                }
                CopyCall::Fence {
                    transfer_id,
                    new_epoch,
                    expect,
                    target_schema_max,
                    holder,
                } => CopyResult::Fence(crate::fence_rules::fence(
                    tx,
                    &transfer_id,
                    new_epoch,
                    &expect,
                    target_schema_max,
                    &holder,
                )?),
                CopyCall::ApplyPiece { piece, sender } => {
                    let (answer, following) =
                        crate::copy_rules::apply_piece(tx, &piece, &sender, now, &mut ids)
                            .map_err(|error| match error {
                                CentralError::Store(StoreError::Full) => CentralError::CopyFull {
                                    call_id: call_id.clone(),
                                },
                                error => error,
                            })?;
                    effects = following;
                    CopyResult::ApplyPiece(answer)
                }
                CopyCall::SnapshotPage { cursor } => CopyResult::SnapshotPage(
                    crate::snapshot_reads::snapshot_page(tx, cursor.as_ref(), now)?,
                ),
            };
            effects.insert(0, CopyEffect::Return { call_id, result });
            Ok(effects)
        }
        CopyInput::CallResult {
            call_id, result, ..
        } => match *result {
            CopyReply::Fleet(FleetResult::DirectoryUpdated) => {
                crate::directory_producer::acknowledged(tx, &call_id)?;
                Ok(Vec::new())
            }
            _ => Err(CentralError::ReplyContract),
        },
        CopyInput::Alarm => copy_alarm_duties(tx, now),
        CopyInput::CallFailed { .. } => Err(CentralError::ReplyContract),
    }
}
fn copy_alarm_duties(
    tx: &mut dyn SqlTx,
    now: RegistryTime,
) -> Result<Vec<CopyEffect>, CentralError> {
    let mut effects = crate::directory_producer::alarm(tx, now)?;
    if crate::registry_dispatch::has_table(tx, "_copy_staged_parts")? {
        crate::copy_receipts::cleanup_parts(tx, now)?;
        if let Some(at) = crate::copy_receipts::next_part_alarm(tx)? {
            effects.push(CopyEffect::SetAlarm { at });
        }
    }
    let earliest = effects
        .iter()
        .filter_map(|effect| match effect {
            CopyEffect::SetAlarm { at } => Some(*at),
            _ => None,
        })
        .min_by_key(|at| at.0);
    effects.retain(|effect| !matches!(effect, CopyEffect::SetAlarm { .. }));
    if let Some(at) = earliest {
        effects.insert(0, CopyEffect::SetAlarm { at });
    }
    Ok(effects)
}
pub fn step_session(
    tx: &mut dyn SqlTx,
    input: SessionInput,
    now: RegistryTime,
    mut ids: IdBatch,
) -> Result<Vec<SessionEffect>, CentralError> {
    match input {
        SessionInput::CallResult {
            call_id,
            result,
            carry,
        } => {
            if carry.is_some() {
                return crate::forward_rules::result(tx, &call_id, &result, carry, now, &mut ids);
            }
            if !crate::registry_dispatch::has_table(tx, "_central_continuations")? {
                return Ok(Vec::new());
            }
            match crate::registry_storage::take_continuation(tx, &call_id)? {
                Some(
                    pending @ (PendingContinuation::SessionRegister { .. }
                    | PendingContinuation::WelcomeCopy { .. }
                    | PendingContinuation::FleetReservation { .. }
                    | PendingContinuation::TailVerification(_)
                    | PendingContinuation::FrameAnswer { .. }),
                ) => crate::session_rules::continuation(tx, pending, *result, now, &mut ids),
                Some(_) => Err(CentralError::ReplyContract),
                None => Ok(Vec::new()),
            }
        }
        SessionInput::CallFailed { call_id, .. } => crate::forward_rules::failed(tx, &call_id, now),
        event => crate::session_rules::input(tx, &event, now, &mut ids),
    }
}
pub fn step_delivery(
    tx: &mut dyn SqlTx,
    input: DeliveryInput,
    now: RegistryTime,
    _ids: IdBatch,
) -> Result<Vec<DeliveryEffect>, CentralError> {
    crate::session_rules::bootstrap(tx)?;
    let mut effects = match input {
        DeliveryInput::Call {
            from,
            call_id,
            call,
        } => match call.as_ref() {
            DeliveryCall::DeliverForward { .. } | DeliveryCall::ForwardAnswered { .. } => {
                crate::forward_delivery::call(tx, &from, &call_id, &call, now)
            }
            DeliveryCall::CloseSession { .. } | DeliveryCall::DeliverNotice(_) => {
                crate::delivery_rules::call(tx, &call_id, &call, now)
            }
        },
        DeliveryInput::Alarm => {
            crate::session_notices::prune(tx, now)?;
            crate::forward_delivery::alarm(tx, now)
        }
        DeliveryInput::CallResult { result, .. } => match *result {},
        DeliveryInput::CallFailed { .. } => Err(CentralError::ReplyContract),
    }?;
    effects.retain(|effect| !matches!(effect, DeliveryEffect::SetAlarm { .. }));
    if let Some(at) = crate::session_notices::delivery_alarm(tx)? {
        effects.push(DeliveryEffect::SetAlarm { at });
    }
    Ok(effects)
}
pub fn step_fleet(
    tx: &mut dyn SqlTx,
    input: FleetInput,
    now: RegistryTime,
    mut ids: IdBatch,
) -> Result<Vec<FleetEffect>, CentralError> {
    let mut effects = match input {
        FleetInput::Call { call_id, call, .. } => {
            crate::fleet_rules::call(tx, &call_id, &call, now)
        }
        FleetInput::Alarm => {
            crate::fleet_storage::bootstrap(tx)?;
            crate::fleet_feed::prune(tx, now)?;
            crate::fleet_notice_outbox::alarm(tx, now, &mut ids)
        }
        FleetInput::CallResult {
            call_id, result, ..
        } => {
            crate::fleet_notice_outbox::acknowledged(tx, &call_id, &result)?;
            Ok(Vec::new())
        }
        FleetInput::CallFailed { .. } => Ok(Vec::new()),
    }?;
    if let Some(at) = crate::fleet_notice_outbox::next_alarm(tx)? {
        effects.push(FleetEffect::SetAlarm { at });
    }
    Ok(effects)
}
