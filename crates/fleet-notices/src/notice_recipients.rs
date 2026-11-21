//! Select explicit and role-based recipients from the drained workspace snapshot.
use central_storage::StoreFailure;
use fleet_runtime_ports::workspace_decode_error::corrupt;
use fleet_workspace_access::{
    workspace_change_records::StoredChange, workspace_row_codec::WorkspaceRows,
};
use std::collections::BTreeSet;
use workspace_model as m;

pub(crate) fn targeted_recipients(
    change: &StoredChange,
    rows: &WorkspaceRows,
) -> Result<BTreeSet<m::ActorIdentityId>, StoreFailure> {
    let mut recipients = BTreeSet::new();
    match change.change()?.event {
        m::ChangeEvent::ProjectRoleGranted { holder, .. }
        | m::ChangeEvent::DeliverableRoleGranted { holder, .. } => {
            recipients.insert(holder);
        }
        m::ChangeEvent::Posted => {
            let m::ItemRef::Post(id) = &change.item else {
                return Err(corrupt("workspace_changes.item_kind"));
            };
            let post = rows
                .posts
                .iter()
                .find(|post| &post.post_id == id)
                .ok_or_else(|| corrupt("workspace_changes.item_id"))?;
            for addressee in &post.addressees {
                match addressee {
                    m::Addressee::Actor { actor_identity_id } => {
                        recipients.insert(actor_identity_id.clone());
                    }
                    m::Addressee::ProjectRole { project_id, role } => {
                        recipients.extend(
                            rows.project_roles
                                .iter()
                                .filter(|assignment| {
                                    &assignment.project_id == project_id
                                        && &assignment.role == role
                                        && assignment.state == m::AssignmentState::Active
                                })
                                .map(|assignment| assignment.holder.clone()),
                        );
                    }
                    m::Addressee::DeliverableRole {
                        deliverable_id,
                        role,
                    } => {
                        recipients.extend(
                            rows.deliverable_roles
                                .iter()
                                .filter(|assignment| {
                                    &assignment.deliverable_id == deliverable_id
                                        && &assignment.role == role
                                        && assignment.state == m::AssignmentState::Active
                                })
                                .map(|assignment| assignment.holder.clone()),
                        );
                    }
                }
            }
        }
        _ => {}
    }
    Ok(recipients)
}
