use crate::sessions::{SessionsProvider, SessionsSort, SessionsSource};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SessionsPickerDataQuery {
    pub root: SessionsPickerRoot,
    pub provider: SessionsProvider,
    pub source: SessionsSource,
    pub sort: SessionsSort,
    pub search: String,
    pub include_empty_sessions: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SessionsPickerRoot {
    Cwd,
    Repo,
    Any,
}
