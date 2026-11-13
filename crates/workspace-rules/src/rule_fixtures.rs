use crate::WriteContext;
use workspace_model::{
    ActorIdentityId, Area, AreaState, Deliverable, Description, Issue, IssueTitle, MemberRole,
    MembershipState, NotificationSetting, Project, ResourceName, Thread, ThreadMember, ThreadState,
    WorkStatus,
};

pub fn id<T: std::str::FromStr>(value: u32) -> T
where
    T::Err: std::fmt::Debug,
{
    format!("01890f2e-7b4c-7cc0-98c4-{value:012x}")
        .parse()
        .unwrap()
}
pub fn name(value: &str) -> ResourceName {
    ResourceName::try_from(value.to_owned()).unwrap()
}
pub fn description(value: &str) -> Description {
    Description::try_from(value.to_owned()).unwrap()
}
pub fn context() -> WriteContext {
    WriteContext {
        workspace_id: id(1),
        actor: id(2),
        change_id: id(3),
        now: "2026-11-13T12:00:00Z".parse().unwrap(),
    }
}
pub fn area() -> Area {
    Area {
        area_id: id(4),
        workspace_id: id(1),
        name: name("Area"),
        state: AreaState::Open,
    }
}
pub fn project() -> Project {
    Project {
        project_id: id(5),
        area_id: id(4),
        title: name("Project X"),
        status: WorkStatus::Open,
        lead: id(2),
    }
}
pub fn deliverable(value: u32) -> Deliverable {
    Deliverable {
        deliverable_id: id(value),
        project_id: id(5),
        title: name(&format!("Deliverable {value}")),
    }
}
pub fn issue(value: u32, deliverable: u32, status: WorkStatus) -> Issue {
    Issue {
        issue_id: id(value),
        deliverable_id: id(deliverable),
        title: IssueTitle::try_from(format!("Issue {value}")).unwrap(),
        status,
    }
}
pub fn thread() -> Thread {
    Thread {
        thread_id: id(8),
        area_id: id(4),
        title: name("Thread"),
        creator: id(2),
        state: ThreadState::Open,
    }
}
pub fn member(actor: ActorIdentityId, role: MemberRole) -> ThreadMember {
    ThreadMember {
        thread_member_id: id(9),
        thread_id: id(8),
        actor_identity_id: actor,
        role,
        notifications: NotificationSetting::On,
        state: MembershipState::Joined,
    }
}
