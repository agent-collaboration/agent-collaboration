//! Standalone argument parsing and native passthrough selection.
use super::{DEFAULT_SESSION_RECORD_LIMIT, SessionsCommandError, SessionsPickerRoot};
use clap::{Parser, ValueEnum};
use std::{
    ffi::{OsStr, OsString},
    str::FromStr,
};

/// Session search root.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SessionsRoot {
    /// Exact current working directory.
    Cwd,
    /// Current Git checkout/worktree root.
    Checkout,
    /// All linked worktrees for the current Git repository.
    Repo,
    /// All known Codex sessions.
    Any,
}

/// Provider filter.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SessionsProvider {
    /// Include all providers.
    Any,
    /// Use the current configured Codex provider.
    Current,
    /// Launch or list Claude Code sessions through Router.
    ClaudeCode,
    /// Match one exact provider id.
    Id(String),
}

impl FromStr for SessionsProvider {
    type Err = String;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        let trimmed = value.trim();
        if trimmed.is_empty() {
            return Err("provider must not be empty".to_owned());
        }
        match trimmed {
            "any" => Ok(Self::Any),
            "current" => Ok(Self::Current),
            "claude" => Ok(Self::ClaudeCode),
            provider_id => Ok(Self::Id(provider_id.to_owned())),
        }
    }
}

impl SessionsProvider {
    pub const fn is_claude_code(&self) -> bool {
        matches!(self, Self::ClaudeCode)
    }
}

/// Session source filter.
#[derive(Clone, Copy, Debug, Eq, PartialEq, ValueEnum)]
#[value(rename_all = "kebab-case")]
pub enum SessionsSource {
    /// Top-level interactive sessions only.
    Interactive,
    /// Include all sources.
    All,
    /// Include subagent sessions only.
    Subagents,
}

/// Session sort order.
#[derive(Clone, Copy, Debug, Eq, PartialEq, ValueEnum)]
#[value(rename_all = "kebab-case")]
pub enum SessionsSort {
    /// Most recently updated first.
    Updated,
    /// Most recently created first.
    Created,
}

/// Sessions output format.
#[derive(Clone, Copy, Debug, Eq, PartialEq, ValueEnum)]
#[value(rename_all = "kebab-case")]
pub enum SessionsFormat {
    /// Human-readable table.
    Table,
    /// JSON records.
    Json,
}

/// Parsed sessions command.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SessionsCommand {
    /// Scope filter.
    pub root: SessionsRoot,
    /// Provider filter.
    pub provider: SessionsProvider,
    /// Source filter.
    pub source: SessionsSource,
    /// Sort order.
    pub sort: SessionsSort,
    /// Render noninteractive list output.
    pub list: bool,
    /// Output format for list mode.
    pub format: SessionsFormat,
    /// Resume the latest session matching filters.
    pub last: bool,
    /// Resume a UUID, Claude background job handle, or unique UUID prefix.
    pub id: Option<String>,
    /// Launch a new session with the explicitly selected provider.
    pub new: bool,
    /// Use the Codex-only local picker (no provider Sessions), then launch Codex locally.
    pub local: bool,
    /// Maximum matching sessions to load.
    pub limit: usize,
    /// Show sessions that have never received a user message.
    pub include_empty_sessions: bool,
    /// Print the command that would be launched instead of executing it.
    pub dry_run: bool,
    /// Arguments passed through to Codex after the router profile is selected.
    pub codex_args: Vec<OsString>,
}

impl From<SessionsPickerRoot> for SessionsRoot {
    fn from(root: SessionsPickerRoot) -> Self {
        match root {
            SessionsPickerRoot::Cwd => Self::Cwd,
            SessionsPickerRoot::Repo => Self::Repo,
            SessionsPickerRoot::Any => Self::Any,
        }
    }
}

impl TryFrom<SessionsRoot> for SessionsPickerRoot {
    type Error = SessionsCommandError;

    fn try_from(root: SessionsRoot) -> Result<Self, Self::Error> {
        match root {
            SessionsRoot::Cwd => Ok(Self::Cwd),
            SessionsRoot::Repo => Ok(Self::Repo),
            SessionsRoot::Any => Ok(Self::Any),
            SessionsRoot::Checkout => Err(SessionsCommandError::InteractiveCheckoutUnsupported),
        }
    }
}

impl SessionsCommand {
    pub fn parse(mut arguments: Vec<OsString>) -> Result<Self, String> {
        let passthrough_separator_index = arguments
            .iter()
            .position(|argument| argument == OsStr::new("--"));
        let first_argument = arguments
            .first()
            .and_then(|argument| argument.to_str())
            .map(str::to_owned);
        if let Some(first_argument) = first_argument
            && validate_exact_uuid_session_id(&first_argument).is_ok()
        {
            let positional_option_end = passthrough_separator_index.unwrap_or(arguments.len());
            let positional_options = arguments.get(1..positional_option_end).unwrap_or_default();
            if contains_explicit_session_id_option(positional_options) {
                return Err("positional session UUID cannot be combined with --id".to_owned());
            }
            arguments.insert(0, OsString::from("--id"));
        }
        let mut argv = Vec::with_capacity(arguments.len() + 1);
        argv.push(OsString::from("sessions"));
        argv.extend(arguments);
        let parsed =
            ClapSessionsCommand::try_parse_from(argv).map_err(|error| error.to_string())?;
        if let Some(session_id) = parsed.id.as_deref() {
            validate_resume_selector(session_id)?;
        }
        if parsed.new && parsed.provider == SessionsProvider::Any {
            return Err("--new requires --provider to choose Codex or Claude".to_owned());
        }

        reject_legacy_router_options(&parsed.codex_args)?;
        reject_interactive_limit(&parsed)?;
        reject_interactive_checkout(&parsed)?;
        let command = Self {
            root: parsed.root()?,
            provider: parsed.provider,
            source: parsed.source,
            sort: parsed.sort,
            list: parsed.list,
            format: parsed.format,
            last: parsed.last,
            id: parsed.id,
            new: parsed.new,
            local: parsed.local,
            limit: parsed.limit.unwrap_or(DEFAULT_SESSION_RECORD_LIMIT),
            include_empty_sessions: parsed.include_empty_sessions,
            dry_run: parsed.dry_run,
            codex_args: parsed.codex_args,
        };
        validate_claude_command(&command)?;
        Ok(command)
    }
}

fn contains_explicit_session_id_option(arguments: &[OsString]) -> bool {
    arguments.iter().any(|argument| {
        argument == OsStr::new("--id")
            || argument
                .to_str()
                .is_some_and(|argument| argument.starts_with("--id="))
    })
}

fn reject_legacy_router_options(codex_args: &[OsString]) -> Result<(), String> {
    if codex_args
        .iter()
        .any(|argument| argument == OsStr::new("--scope"))
    {
        return Err("--scope was removed; use --checkout, --repo, or --any".to_owned());
    }
    Ok(())
}

fn validate_resume_selector(selector: &str) -> Result<(), String> {
    if validate_exact_uuid_session_id(selector).is_ok() {
        return Ok(());
    }
    let hex = selector.bytes().filter(|byte| *byte != b'-').count();
    let compact = !selector.contains('-') && (8..=32).contains(&selector.len());
    let canonical_prefix = (8..36).contains(&selector.len())
        && selector.bytes().enumerate().all(|(index, byte)| {
            if matches!(index, 8 | 13 | 18 | 23) {
                byte == b'-'
            } else {
                byte.is_ascii_hexdigit()
            }
        });
    if (8..=32).contains(&hex)
        && (compact || canonical_prefix)
        && selector
            .bytes()
            .all(|byte| byte == b'-' || byte.is_ascii_hexdigit())
    {
        return Ok(());
    }
    Err("--id requires a complete UUID, a Claude job handle, or a UUID prefix of at least 8 hexadecimal characters".to_owned())
}

fn reject_interactive_limit(command: &ClapSessionsCommand) -> Result<(), String> {
    if command.limit.is_some() && !command.list {
        return Err("--limit only applies with --list".to_owned());
    }
    Ok(())
}

fn reject_interactive_checkout(command: &ClapSessionsCommand) -> Result<(), String> {
    let opens_picker = !command.list && !command.last && command.id.is_none() && !command.new;
    if command.checkout && opens_picker {
        return Err(
            "--checkout requires --list because the interactive picker supports cwd, repo, and all"
                .to_owned(),
        );
    }
    Ok(())
}

pub(crate) fn validate_claude_command(command: &SessionsCommand) -> Result<(), String> {
    if command.provider != SessionsProvider::ClaudeCode {
        return Ok(());
    }
    if command.local {
        return Err("--local is only supported for Codex sessions".to_owned());
    }
    if command.last {
        return Err("--last is not supported for Claude sessions; use --id <uuid>".to_owned());
    }
    if !command.list && !command.new && command.id.is_none() {
        return Err("--provider claude requires --list, --new, or --id <uuid>".to_owned());
    }
    if command.root != SessionsRoot::Cwd {
        return Err("Claude session commands do not accept Codex root filters".to_owned());
    }
    if command.source != SessionsSource::Interactive {
        return Err("Claude sessions only support --source interactive".to_owned());
    }
    if command.sort != SessionsSort::Updated {
        return Err("Claude sessions only support --sort updated".to_owned());
    }
    if command.include_empty_sessions {
        return Err("--include-empty-sessions is only supported for Codex sessions".to_owned());
    }
    Ok(())
}

pub fn validate_exact_uuid_session_id(session_id: &str) -> Result<(), String> {
    let bytes = session_id.as_bytes();
    let is_canonical_uuid = bytes.len() == 36
        && [8, 13, 18, 23]
            .into_iter()
            .all(|index| bytes.get(index) == Some(&b'-'))
        && bytes
            .iter()
            .enumerate()
            .all(|(index, byte)| matches!(index, 8 | 13 | 18 | 23) || byte.is_ascii_hexdigit());
    if !is_canonical_uuid {
        return Err("--id requires a complete UUID".to_owned());
    }
    Ok(())
}

#[derive(Debug, Parser)]
#[command(name = "sessions", disable_help_subcommand = true)]
struct ClapSessionsCommand {
    #[arg(long, conflicts_with_all = ["repo", "any"])]
    checkout: bool,
    #[arg(long, conflicts_with_all = ["checkout", "any"])]
    repo: bool,
    #[arg(long, conflicts_with_all = ["checkout", "repo"])]
    any: bool,
    #[arg(
        long,
        default_value = "any",
        help = "Codex model-provider filter, or claude; --id detects the agent automatically"
    )]
    provider: SessionsProvider,
    #[arg(long, value_enum, default_value = "interactive")]
    source: SessionsSource,
    #[arg(long, value_enum, default_value = "updated")]
    sort: SessionsSort,
    #[arg(long)]
    list: bool,
    #[arg(long, value_enum, default_value = "table")]
    format: SessionsFormat,
    #[arg(long, conflicts_with = "list")]
    last: bool,
    /// Resume a UUID, Claude job handle, or unique UUID prefix (at least 8 hex digits).
    #[arg(long, conflicts_with_all = ["new", "last", "list"])]
    id: Option<String>,
    #[arg(long, conflicts_with_all = ["list", "last"])]
    new: bool,
    #[arg(long)]
    local: bool,
    #[arg(long)]
    limit: Option<usize>,
    /// Show sessions that have never received a user message.
    #[arg(long)]
    include_empty_sessions: bool,
    #[arg(long)]
    dry_run: bool,
    #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
    codex_args: Vec<OsString>,
}

impl ClapSessionsCommand {
    fn root(&self) -> Result<SessionsRoot, String> {
        match (self.checkout, self.repo, self.any) {
            (true, false, false) => Ok(SessionsRoot::Checkout),
            (false, true, false) => Ok(SessionsRoot::Repo),
            (false, false, true) => Ok(SessionsRoot::Any),
            (false, false, false) => Ok(SessionsRoot::Cwd),
            _ => Err("--checkout, --repo, and --any cannot be used together".to_owned()),
        }
    }
}

/// Renders the parser-owned public help text without starting native work.
pub fn command_help(executable_name: &'static str) -> String {
    use clap::CommandFactory;
    ClapSessionsCommand::command()
        .name(executable_name)
        .render_long_help()
        .to_string()
}

#[cfg(test)]
#[path = "session_command_options_test.rs"]
mod session_command_options_test;
