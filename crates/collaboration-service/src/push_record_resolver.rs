//! Resolves local push links, applies reported-caller access rules, and expands stored ranges.
use collaboration_contracts::RouterIdentity;
use collaboration_protocol::{
    MachineId, PushId, PushKind, PushLineInput, PushOrigin, PushRecord, PushRecordNotice,
    RouterLink, SessionRef, render_push_line,
};

pub(crate) fn link_for(record: &PushRecord, identity: &RouterIdentity) -> String {
    RouterLink::new(
        MachineId::from(identity.machine_identity().service_id().clone()),
        record.push_id.clone(),
    )
    .to_string()
}

pub(crate) fn line_for(
    record: &PushRecord,
    identity: &RouterIdentity,
) -> Result<String, collaboration_protocol::PushLineError> {
    render_push_line(&PushLineInput {
        link: RouterLink::new(
            MachineId::from(identity.machine_identity().service_id().clone()),
            record.push_id.clone(),
        ),
        machine_label: identity.machine_identity().machine_label().clone(),
        origin: record.origin.clone(),
        header_facts: record.header_facts.clone(),
        body: record.body.clone(),
    })
}

pub(crate) fn resolve_reference(
    reference: &str,
    identity: &RouterIdentity,
) -> Result<PushId, ReferenceError> {
    if let Ok(push_id) = PushId::try_from(reference.to_owned()) {
        return Ok(push_id);
    }
    let link = RouterLink::parse(reference).map_err(|_| ReferenceError::Invalid)?;
    let local_machine_id = String::from(identity.machine_identity().service_id().clone());
    if link.machine_id().as_str() != local_machine_id {
        return Err(ReferenceError::Foreign(
            link.machine_id().as_str().to_owned(),
        ));
    }
    Ok(link.push_id().clone())
}

pub(crate) fn can_read(record: &PushRecord, caller: &SessionRef) -> bool {
    if &record.target == caller {
        return true;
    }
    record.kind == PushKind::DirectMessage
        && matches!(&record.origin, PushOrigin::Session(origin) if origin == caller)
}

pub(crate) fn caller_is_local(caller: &SessionRef, identity: &RouterIdentity) -> bool {
    caller.endpoint.service_id == *identity.service_id()
}

pub(crate) fn make_notice_list(
    records: Vec<PushRecord>,
    identity: &RouterIdentity,
) -> Result<Vec<PushRecordNotice>, collaboration_protocol::PushLineError> {
    records
        .into_iter()
        .map(|record| {
            Ok(PushRecordNotice {
                push_id: record.push_id.clone(),
                link: link_for(&record, identity),
                line: line_for(&record, identity)?,
                origin: record.origin.clone(),
                target: record.target.clone(),
                reply_to_push_id: record.reply_to_push_id.clone(),
                delivery_state: record.delivery_state,
                cancelled_after_delivery: record.cancelled_after_delivery.clone(),
                created_at: record.created_at,
                read_at: record.read_at,
            })
        })
        .collect()
}

pub(crate) enum ReferenceError {
    Invalid,
    Foreign(String),
}
