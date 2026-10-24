//! Shared real-file setups and evidence builders for the oracle tests.
use crate::catalogue_oracle::NativeSkillAuthority;
use crate::verification_models::{
    FixtureRecord, HistoryThread, OperationRow, ProviderEndpoint, ProviderObservation, RpcReply,
};
use serde_json::{Value, json};
use std::{
    fs,
    path::{Path, PathBuf},
};
use tempfile::TempDir;

pub(crate) const ARGUMENTS: &str = "literal-$value café\nsecond-line";
pub(crate) const CANARY: &str = "NATIVE_SKILL_0123456789ABCDEF";
pub(crate) const SECOND_CANARY: &str = "NATIVE_SKILL_FEDCBA9876543210";
pub(crate) const SKILL: &str = "router-native-arguments";
pub(crate) const SECOND_SKILL: &str = "router-native-second";

/// A wrong build must fail, and for the stated reason.
pub(crate) fn expect_error<T: std::fmt::Debug>(result: Result<T, String>, fragment: &str) {
    let error = result.expect_err("a wrong build must fail");
    assert!(
        error.contains(fragment),
        "{error:?} does not mention {fragment:?}"
    );
}

pub(crate) fn write_skill(root: &Path, name: &str, extra: &str) -> PathBuf {
    let path = root.join(name).join("SKILL.md");
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(
        &path,
        format!("---\nname: {name}\ndescription: {name} skill\n{extra}---\nBody\n"),
    )
    .unwrap();
    path
}

/// Real files for one fixture workspace, an empty workspace and native home roots.
pub(crate) struct NativeSetup {
    _directory: TempDir,
    pub(crate) workspace: PathBuf,
    pub(crate) empty: PathBuf,
    pub(crate) home: PathBuf,
    pub(crate) skill_file: PathBuf,
    pub(crate) second_file: PathBuf,
    pub(crate) user_skill: PathBuf,
    /// Cursor's own loader reads ~/.codex/skills; a provider that loads it may report it.
    pub(crate) shared_skill: PathBuf,
    pub(crate) codex_only: PathBuf,
    pub(crate) fixture: FixtureRecord,
}

impl NativeSetup {
    pub(crate) fn new() -> Self {
        let directory = TempDir::new().unwrap();
        let root = fs::canonicalize(directory.path()).unwrap();
        let workspace = root.join("workspace");
        let empty = root.join("empty");
        let home = root.join("home");
        fs::create_dir_all(&empty).unwrap();
        let skills = workspace.join(".claude/skills");
        let skill_file = write_skill(&skills, SKILL, "");
        let second_file = write_skill(&skills, SECOND_SKILL, "");
        let user_skill = write_skill(&home.join(".claude/skills"), "user-native", "");
        let shared_skill = write_skill(&home.join(".codex/skills"), "shared-native", "");
        let codex_only = write_skill(&home.join(".codex/skills"), "codex-only", "");
        let fixture = FixtureRecord {
            workspace: workspace.clone(),
            workspace_head: "0".repeat(40),
            empty_workspace: empty.clone(),
            visible_file: workspace.join("README.verification.txt"),
            skill_name: SKILL.to_owned(),
            skill_file: skill_file.clone(),
            canary: CANARY.to_owned(),
            second_skill_name: SECOND_SKILL.to_owned(),
            second_skill_file: second_file.clone(),
            second_canary: SECOND_CANARY.to_owned(),
        };
        Self {
            _directory: directory,
            workspace,
            empty,
            home,
            skill_file,
            second_file,
            user_skill,
            shared_skill,
            codex_only,
            fixture,
        }
    }

    pub(crate) fn authority(
        &self,
        provider: ProviderEndpoint,
        extra: &[&str],
        shared_root: bool,
    ) -> NativeSkillAuthority {
        let mut home_skill_roots = vec![self.home.join(".claude/skills")];
        if shared_root {
            home_skill_roots.push(self.home.join(".codex/skills"));
        }
        let mut advertised: Vec<String> = [SKILL, SECOND_SKILL, "user-native"]
            .map(str::to_owned)
            .to_vec();
        advertised.extend(extra.iter().map(|name| (*name).to_owned()));
        NativeSkillAuthority {
            provider,
            advertised_names: advertised.into_iter().collect(),
            home_skill_roots,
            workspace_skill_dirs: vec![".claude/skills".to_owned()],
            home_command_roots: Vec::new(),
            workspace_command_dirs: Vec::new(),
            project_root_marker: None,
            plugin_namespaces: Vec::new(),
        }
    }
}

pub(crate) fn skill_row(name: &str, path: &Path, scope: &str, enabled: bool) -> Value {
    json!({"name": name, "description": "d", "path": path, "scope": scope, "enabled": enabled})
}

pub(crate) fn skills_reply(cwd: &Path, skills: Vec<Value>, errors: Vec<Value>) -> RpcReply {
    serde_json::from_value(json!({
        "id": 2,
        "result": {"data": [{"cwd": cwd, "skills": skills, "errors": errors}]},
    }))
    .unwrap()
}

pub(crate) fn error_reply() -> RpcReply {
    serde_json::from_value(json!({"id": 2, "error": {"code": -32000, "message": "unsupported"}}))
        .unwrap()
}

pub(crate) fn result_reply(result: Value) -> RpcReply {
    serde_json::from_value(json!({"id": 3, "result": result})).unwrap()
}

pub(crate) fn observation(events: Vec<Value>, end: &str) -> ProviderObservation {
    let sequenced: Vec<Value> = events
        .into_iter()
        .enumerate()
        .map(|(index, event)| json!({"sequence": index + 1, "event": event}))
        .collect();
    serde_json::from_value(json!({
        "target": {"endpoint": {"serviceId": "service", "endpointId": "claude-local"}, "sessionId": "thread"},
        "events": sequenced,
        "epoch": 0,
        "endReason": end,
    }))
    .unwrap()
}

pub(crate) fn item(item_id: &str, kind: &str, text: &str) -> Value {
    json!({"kind": "itemStarted", "item": {"itemId": item_id, "kind": {"kind": kind}, "text": text}})
}

pub(crate) fn catalogue(item_id: &str, names: &[&str]) -> Value {
    let commands: Vec<Value> = names
        .iter()
        .map(|name| json!({"name": name, "description": ""}))
        .collect();
    item(
        item_id,
        "notice",
        &json!({"availableCommands": commands}).to_string(),
    )
}

pub(crate) fn thread(id: &str, items: Vec<Value>) -> HistoryThread {
    serde_json::from_value(json!({"id": id, "turns": [{"id": "turn", "items": items}]})).unwrap()
}

pub(crate) fn user(id: &str, text: &str) -> Value {
    json!({"id": id, "type": "userMessage", "content": [{"type": "text", "text": text}]})
}

pub(crate) fn agent(id: &str, text: &str) -> Value {
    json!({"id": id, "type": "agentMessage", "text": text})
}

pub(crate) fn turn_started(turn_id: &str) -> Value {
    json!({"kind": "turnStarted", "turnId": turn_id})
}

pub(crate) fn operation(
    operation_id: &str,
    kind: &str,
    admitted: i64,
    terminal: Option<i64>,
) -> OperationRow {
    OperationRow {
        operation_id: operation_id.to_owned(),
        operation_kind: kind.to_owned(),
        stage: "terminal".to_owned(),
        admitted_at_ms: admitted,
        terminal_at_ms: terminal,
    }
}
