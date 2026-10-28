//! CLI conversion into the reusable stored-session catalog.

use super::{
    DEFAULT_SESSION_RECORD_LIMIT, RepositoryIdentity, SessionRecord, SessionsCommand,
    SessionsCommandError, SessionsPickerDataQuery, SessionsProvider, SessionsRoot, SessionsSort,
    SessionsSource,
};
use collaboration_client::session_catalog::{
    SessionCatalogProvider, SessionCatalogQuery, SessionCatalogRoot, SessionCatalogSort,
    SessionCatalogSource, load_stored_sessions,
};
use std::path::Path;

/// A session-id search is a substring match, so read a few rows and keep the exact one.
const SESSION_ID_LOOKUP_LIMIT: usize = 8;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SessionRecordQuery {
    root: SessionCatalogRoot,
    provider: SessionCatalogProvider,
    source: SessionCatalogSource,
    sort: SessionCatalogSort,
    last: bool,
    include_empty_sessions: bool,
    limit: usize,
    search: String,
}

impl SessionRecordQuery {
    pub fn from_command(command: &SessionsCommand) -> Self {
        Self {
            root: catalog_root(command.root),
            provider: catalog_provider(&command.provider),
            source: catalog_source(command.source),
            sort: catalog_sort(command.sort),
            last: command.last,
            include_empty_sessions: command.include_empty_sessions,
            limit: command.limit,
            search: String::new(),
        }
    }

    /// Query that isolates one stored record by its session id.
    pub fn for_session_id(session_id: &str) -> Self {
        Self {
            root: SessionCatalogRoot::Any,
            provider: SessionCatalogProvider::Any,
            source: SessionCatalogSource::All,
            sort: SessionCatalogSort::Updated,
            last: false,
            include_empty_sessions: true,
            limit: SESSION_ID_LOOKUP_LIMIT,
            search: format!("id:{session_id}"),
        }
    }

    pub fn from_picker_query(query: SessionsPickerDataQuery) -> Self {
        Self {
            root: catalog_root(query.root.into()),
            provider: catalog_provider(&query.provider),
            source: catalog_source(query.source),
            sort: catalog_sort(query.sort),
            last: false,
            include_empty_sessions: query.include_empty_sessions,
            limit: DEFAULT_SESSION_RECORD_LIMIT,
            search: query.search,
        }
    }
}

pub async fn load_session_records(
    command: SessionsCommand,
    codex_home: &Path,
    current_dir: &Path,
) -> Result<Vec<SessionRecord>, SessionsCommandError> {
    load_session_records_for_query_with_identity(
        SessionRecordQuery::from_command(&command),
        codex_home,
        current_dir,
        None,
    )
    .await
}

pub async fn load_session_records_for_query_with_identity(
    query: SessionRecordQuery,
    codex_home: &Path,
    current_dir: &Path,
    repository_identity: Option<RepositoryIdentity>,
) -> Result<Vec<SessionRecord>, SessionsCommandError> {
    Ok(load_stored_sessions(SessionCatalogQuery {
        codex_home: codex_home.to_path_buf(),
        current_dir: current_dir.to_path_buf(),
        root: query.root,
        provider: query.provider,
        source: query.source,
        sort: query.sort,
        last: query.last,
        include_empty_sessions: query.include_empty_sessions,
        limit: query.limit,
        search: query.search,
        repository_identity,
    })
    .await?)
}

fn catalog_root(root: SessionsRoot) -> SessionCatalogRoot {
    match root {
        SessionsRoot::Cwd => SessionCatalogRoot::Cwd,
        SessionsRoot::Checkout => SessionCatalogRoot::Checkout,
        SessionsRoot::Repo => SessionCatalogRoot::Repository,
        SessionsRoot::Any => SessionCatalogRoot::Any,
    }
}

fn catalog_provider(provider: &SessionsProvider) -> SessionCatalogProvider {
    match provider {
        SessionsProvider::Any => SessionCatalogProvider::Any,
        SessionsProvider::Current => SessionCatalogProvider::Current,
        SessionsProvider::ClaudeCode => SessionCatalogProvider::Id("claude".to_owned()),
        SessionsProvider::Id(provider_id) => SessionCatalogProvider::Id(provider_id.clone()),
    }
}

fn catalog_source(source: SessionsSource) -> SessionCatalogSource {
    match source {
        SessionsSource::Interactive => SessionCatalogSource::Interactive,
        SessionsSource::All => SessionCatalogSource::All,
        SessionsSource::Subagents => SessionCatalogSource::Subagents,
    }
}

fn catalog_sort(sort: SessionsSort) -> SessionCatalogSort {
    match sort {
        SessionsSort::Updated => SessionCatalogSort::Updated,
        SessionsSort::Created => SessionCatalogSort::Created,
    }
}
