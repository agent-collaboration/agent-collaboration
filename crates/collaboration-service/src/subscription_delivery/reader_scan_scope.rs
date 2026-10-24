//! A pending pass carries explicit dirty projects; a lost signal requires discovery again.
use message_board::ProjectId;
use std::collections::BTreeSet;

#[derive(Clone)]
pub(super) enum ReaderScanScope {
    All,
    Projects(BTreeSet<ProjectId>),
}
impl ReaderScanScope {
    pub fn project(project: ProjectId) -> Self {
        Self::Projects(BTreeSet::from([project]))
    }
    pub fn merge(&mut self, requested: Self) {
        match (self, requested) {
            (current @ Self::Projects(_), Self::All) => *current = Self::All,
            (Self::Projects(current), Self::Projects(projects)) => current.extend(projects),
            (Self::All, _) => {}
        }
    }
}
