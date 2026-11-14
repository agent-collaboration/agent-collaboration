use crate::workspace_decode_error::corrupt;
use central_storage::StoreFailure;
use serde::Deserialize;
use workspace_model::{
    ActorIdentityId, Change, ChangeEvent, ChangeId, ChangePosition, ItemRef, ProjectId, RoleName,
    ThreadId,
};
#[derive(Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct StoredChange {
    pub change_id: ChangeId,
    pub position: ChangePosition,
    pub item: ItemRef,
    pub actor: ActorIdentityId,
    pub change_kind: String,
    pub thread_id: Option<ThreadId>,
    pub project_id: Option<ProjectId>,
    pub subject_actor: Option<ActorIdentityId>,
    pub role: Option<String>,
    #[serde(default)]
    pub created_at: Option<workspace_model::Timestamp>,
}
impl StoredChange {
    pub fn change(&self) -> Result<Change, StoreFailure> {
        if let Some(project) = &self.project_id
            && let ItemRef::Project(item) = &self.item
            && item != project
        {
            return Err(corrupt("changes.project_id"));
        }
        let member = || {
            self.subject_actor
                .clone()
                .ok_or_else(|| corrupt("subject_actor"))
        };
        let event = match self.change_kind.as_str() {
            "created" => ChangeEvent::Created,
            "updated" => ChangeEvent::Updated,
            "removed" => ChangeEvent::Removed,
            "posted" => ChangeEvent::Posted,
            "resolved" => ChangeEvent::Resolved,
            "redacted" => ChangeEvent::Redacted,
            "stateChanged" => ChangeEvent::StateChanged,
            "joined" => ChangeEvent::Joined { member: member()? },
            "left" => ChangeEvent::Left { member: member()? },
            "notificationsOn" => ChangeEvent::NotificationsOn { member: member()? },
            "notificationsOff" => ChangeEvent::NotificationsOff { member: member()? },
            "roleGranted" | "roleEnded" => {
                let role = self.role.as_ref().ok_or_else(|| corrupt("role"))?;
                let granted = self.change_kind == "roleGranted";
                let role: RoleName = match self.item {
                    ItemRef::Project(_) => RoleName::Project(
                        serde_json::from_value(serde_json::Value::String(role.clone()))
                            .map_err(|_| corrupt("project_roles.role"))?,
                    ),
                    ItemRef::Deliverable(_) => RoleName::Deliverable(
                        serde_json::from_value(serde_json::Value::String(role.clone()))
                            .map_err(|_| corrupt("deliverable_roles.role"))?,
                    ),
                    _ => return Err(corrupt("role item")),
                };
                match (role, granted) {
                    (RoleName::Project(role), true) => ChangeEvent::ProjectRoleGranted {
                        holder: member()?,
                        role,
                    },
                    (RoleName::Project(role), false) => ChangeEvent::ProjectRoleEnded {
                        holder: member()?,
                        role,
                    },
                    (RoleName::Deliverable(role), true) => ChangeEvent::DeliverableRoleGranted {
                        holder: member()?,
                        role,
                    },
                    (RoleName::Deliverable(role), false) => ChangeEvent::DeliverableRoleEnded {
                        holder: member()?,
                        role,
                    },
                }
            }
            _ => return Err(corrupt("change_kind")),
        };
        Ok(Change {
            change_id: self.change_id.clone(),
            position: self.position,
            item: self.item.clone(),
            actor: self.actor.clone(),
            event,
        })
    }
}
