//! Command-line shape of the native-capabilities verifier, parsed by hand like the
//! other Host examples. Unknown, repeated or missing options fail before any work.
use crate::native_journeys::SelectionCase;
use std::{collections::BTreeMap, path::PathBuf};

pub(crate) const USAGE: &str = "\
Drive and read back the native-capabilities journeys J1-J4 on a declared candidate.
Journey: .agents/skills/verify-codex-router/references/native-capabilities.md

Usage: scripts/verification/native-capabilities.sh <command> [options]

  materialize --workspace W --empty-workspace E --record FIXTURE.json
      Create owned workspaces with the controlled Claude project skills and fresh canaries.
  catalogue PLAN --output DIR
      J1 discovery, wrong context, plugins, management rejection, `$` and `@` popups.
  select PLAN --case CASE --arguments TEXT --output DIR
      J2 installed-Codex `$` selection. CASE: leading (positive), nonleading,
      two-skills, selected-steer (explicit rejections).
  commands PLAN --arguments TEXT --output DIR [--reload-probe]
      J3 catalogue replacement, Router `/` invocation, installed-Codex picker refusal.
      --reload-probe then removes the second fixture skill (exact recorded bytes),
      sends /reload-skills through Router, observes the lists and restores the file.
  history PLAN --arguments TEXT --output DIR --ordinary-live-capture PTY
          --ordinary-live-offset BYTES [--unloaded-resume]
      J4 read/resume canonical history, display operations, client reopen and the
      ACP metadata oracle. PTY is an original live client capture on the same
      Session that actually drew the ordinary STEER_BASE_OK reply. The selected-steer
      run's select-selected-steer.pty qualifies only if it drew that reply; otherwise
      use a separate genuine live ordinary control. Reopened history is never live.
      BYTES is the raw capture length recorded just before the ordinary input; only
      the bytes after it can show the live reply (a resumed client redraws history
      first). The whole capture is still scanned for metadata leaks.
  processes capture --root-pid PID --output FILE
      Record the owned process closure of a Lead-started Router or Host (observe only).
  processes check --inventory FILE
      Report recorded processes still present; exit 1 when any remain. Never signals.

PLAN is a NativeCapabilitiesPlan JSON for the candidate the Lead started through the
ordinary `codex-router serve`/`host` lifetimes. Qualification (clean declared commit,
binary hashes, debug signatures, live real-UID Router/Host identities, version,
published endpoint, fixture) runs before any drive and fails closed. Each run writes
raw evidence to a fresh private DIR and prints a typed result. Exit 1 means the oracle
rejected the evidence or the run broke: read DIR/failure.txt. The verifier never
creates Sessions, logs in, edits installations or stops a Host or Router.
";

#[derive(Debug, Eq, PartialEq)]
pub(crate) enum VerifierCommand {
    Help,
    Materialize {
        workspace: PathBuf,
        empty_workspace: PathBuf,
        record: PathBuf,
    },
    Catalogue {
        plan: PathBuf,
        output: PathBuf,
    },
    Select {
        plan: PathBuf,
        output: PathBuf,
        case: SelectionCase,
        arguments: String,
    },
    Commands {
        plan: PathBuf,
        output: PathBuf,
        arguments: String,
        reload_probe: bool,
    },
    History {
        plan: PathBuf,
        output: PathBuf,
        arguments: String,
        unloaded_resume: bool,
        ordinary_live_capture: PathBuf,
        /// Raw capture bytes recorded before the ordinary input; required, no default.
        ordinary_live_offset: usize,
    },
    CaptureProcesses {
        root_pid: u32,
        output: PathBuf,
    },
    CheckProcesses {
        inventory: PathBuf,
    },
}

/// Positional words and `--name value` options; `flags` take no value.
struct ParsedOptions {
    positional: Vec<String>,
    values: BTreeMap<String, String>,
    flags: Vec<String>,
}

impl ParsedOptions {
    fn parse(words: &[String], valued: &[&str], flags: &[&str]) -> Result<Self, String> {
        let mut parsed = Self {
            positional: Vec::new(),
            values: BTreeMap::new(),
            flags: Vec::new(),
        };
        let mut words = words.iter();
        while let Some(word) = words.next() {
            let Some(name) = word.strip_prefix("--") else {
                parsed.positional.push(word.clone());
                continue;
            };
            if flags.contains(&name) {
                if parsed.flags.iter().any(|seen| seen == name) {
                    return Err(format!("--{name} given twice"));
                }
                parsed.flags.push(name.to_owned());
            } else if valued.contains(&name) {
                let value = words
                    .next()
                    .ok_or_else(|| format!("--{name} needs a value"))?;
                if parsed
                    .values
                    .insert(name.to_owned(), value.clone())
                    .is_some()
                {
                    return Err(format!("--{name} given twice"));
                }
            } else {
                return Err(format!("unknown option --{name}"));
            }
        }
        Ok(parsed)
    }

    fn required(&mut self, name: &str) -> Result<String, String> {
        self.values
            .remove(name)
            .ok_or_else(|| format!("--{name} is required"))
    }

    fn flag(&self, name: &str) -> bool {
        self.flags.iter().any(|seen| seen == name)
    }

    fn only_plan(&mut self) -> Result<PathBuf, String> {
        match self.positional.as_slice() {
            [plan] => Ok(PathBuf::from(plan)),
            _ => Err("exactly one PLAN path is required".to_owned()),
        }
    }
}

pub(crate) fn parse_command(words: &[String]) -> Result<VerifierCommand, String> {
    let Some((command, rest)) = words.split_first() else {
        return Err("a command is required; see --help".to_owned());
    };
    if rest.iter().any(|word| word == "--help" || word == "-h")
        || command == "--help"
        || command == "-h"
    {
        return Ok(VerifierCommand::Help);
    }
    match command.as_str() {
        "materialize" => {
            let mut options =
                ParsedOptions::parse(rest, &["workspace", "empty-workspace", "record"], &[])?;
            if !options.positional.is_empty() {
                return Err("materialize takes no positional arguments".to_owned());
            }
            Ok(VerifierCommand::Materialize {
                workspace: options.required("workspace")?.into(),
                empty_workspace: options.required("empty-workspace")?.into(),
                record: options.required("record")?.into(),
            })
        }
        "catalogue" => {
            let mut options = ParsedOptions::parse(rest, &["output"], &[])?;
            Ok(VerifierCommand::Catalogue {
                plan: options.only_plan()?,
                output: options.required("output")?.into(),
            })
        }
        "select" => {
            let mut options = ParsedOptions::parse(rest, &["output", "case", "arguments"], &[])?;
            Ok(VerifierCommand::Select {
                plan: options.only_plan()?,
                output: options.required("output")?.into(),
                case: SelectionCase::parse(&options.required("case")?)?,
                arguments: options.required("arguments")?,
            })
        }
        "commands" => {
            let mut options =
                ParsedOptions::parse(rest, &["output", "arguments"], &["reload-probe"])?;
            Ok(VerifierCommand::Commands {
                plan: options.only_plan()?,
                output: options.required("output")?.into(),
                arguments: options.required("arguments")?,
                reload_probe: options.flag("reload-probe"),
            })
        }
        "history" => {
            let mut options = ParsedOptions::parse(
                rest,
                &[
                    "output",
                    "arguments",
                    "ordinary-live-capture",
                    "ordinary-live-offset",
                ],
                &["unloaded-resume"],
            )?;
            Ok(VerifierCommand::History {
                plan: options.only_plan()?,
                output: options.required("output")?.into(),
                arguments: options.required("arguments")?,
                unloaded_resume: options.flag("unloaded-resume"),
                ordinary_live_capture: options.required("ordinary-live-capture")?.into(),
                ordinary_live_offset: options
                    .required("ordinary-live-offset")?
                    .parse()
                    .map_err(|_| "--ordinary-live-offset must be a byte count".to_owned())?,
            })
        }
        "processes" => {
            let (action, rest) = rest
                .split_first()
                .ok_or("processes takes capture or check")?;
            let valued: &[&str] = match action.as_str() {
                "capture" => &["root-pid", "output"],
                "check" => &["inventory"],
                _ => return Err("processes takes capture or check".to_owned()),
            };
            let mut options = ParsedOptions::parse(rest, valued, &[])?;
            if !options.positional.is_empty() {
                return Err(format!("processes {action} takes no positional arguments"));
            }
            if action == "capture" {
                Ok(VerifierCommand::CaptureProcesses {
                    root_pid: options
                        .required("root-pid")?
                        .parse()
                        .map_err(|_| "--root-pid must be a process ID".to_owned())?,
                    output: options.required("output")?.into(),
                })
            } else {
                Ok(VerifierCommand::CheckProcesses {
                    inventory: options.required("inventory")?.into(),
                })
            }
        }
        other => Err(format!("unknown command {other:?}; see --help")),
    }
}
