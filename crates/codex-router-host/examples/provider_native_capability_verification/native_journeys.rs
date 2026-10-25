//! Journeys J1-J4 (Verification Design): each drives the real installed client or
//! the Router's public surfaces, then hands raw evidence to an independent oracle.
use crate::catalogue_oracle::{
    NativeSkillAuthority, PluginCatalogueCheck, check_installed_plugin_skill_sample,
    check_management_rejected, check_plugin_catalogue, check_plugin_detail, check_skill_catalogue,
    check_wrong_context_catalogue,
};
use crate::command_reload_probe::reload_after_fixture_removal;
use crate::conversation_oracle::{
    QueueObservation, advertised_command_names, any_turn_ended, check_command_catalogues,
    check_display_operations, check_native_invocation, check_ordinary_completion,
    check_picker_refusal, check_recorded_selection, check_rejected_input, check_selected_history,
    expected_native_reply, new_operations, require_fresh_session, selected_steer_raced,
};
use crate::installed_client_terminal::{CAPTURE_MAX_BYTES, CLEAR_LINE, ESCAPE, terminal_text};
use crate::journey_run::{Journey, JourneyResult, PICKER_SECONDS, json_value, name_fragment};
use crate::metadata_oracle::check_metadata_observation;
use crate::native_exports::{read_bounded_bytes, read_session_recorder};
use crate::native_file_facts::{native_plugin_facts, plugin_namespace, plugin_skill_files};
use crate::native_fixture::write_private;
use crate::verification_models::{
    CapabilityStatus, HistoryThread, NativePluginRow, ObservationCursor, PluginListView,
    ProviderObservation, RpcReply,
};
use serde_json::{Map, Value, json};
use std::{
    path::Path,
    time::{Duration, Instant},
};

const COMPOSER_READY: &str = "›";
const PICKER_ROW: &str = "echoes its arguments";
const SECOND_PICKER_ROW: &str = "must be rejected";
const STARTUP: Duration = Duration::from_secs(90);
/// How long the ready composer is watched for a following trust dialog before input.
const COMPOSER_SETTLE: Duration = Duration::from_secs(5);
const REPLY: Duration = Duration::from_secs(180);
const REJECTION: Duration = Duration::from_secs(60);
/// The ordinary turn a selected steer targets; its reply is absent from the prompt.
pub(crate) const STEER_BASE_PROMPT: &str = "Reply with exactly three uppercase words joined by underscores: STEER, BASE, OK. Do not use tools.";
pub(crate) const STEER_BASE_REPLY: &str = "STEER_BASE_OK";
/// A plugin the provider cannot have, so even a wrongly dispatched install installs nothing.
const ABSENT_PLUGIN: &str = "router-verifier-absent-plugin";

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum SelectionCase {
    Leading,
    Nonleading,
    TwoSkills,
    SelectedSteer,
}

impl SelectionCase {
    pub(crate) fn parse(text: &str) -> Result<Self, String> {
        match text {
            "leading" => Ok(Self::Leading),
            "nonleading" => Ok(Self::Nonleading),
            "two-skills" => Ok(Self::TwoSkills),
            "selected-steer" => Ok(Self::SelectedSteer),
            other => Err(format!(
                "unknown case {other:?}: leading, nonleading, two-skills or selected-steer"
            )),
        }
    }

    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::Leading => "leading",
            Self::Nonleading => "nonleading",
            Self::TwoSkills => "two-skills",
            Self::SelectedSteer => "selected-steer",
        }
    }
}

fn four_replies(replies: Vec<RpcReply>) -> Result<[RpcReply; 4], String> {
    <[RpcReply; 4]>::try_from(replies)
        .map_err(|_| "consumer requests returned a different reply count".to_owned())
}

/// Plugin list, management rejection and one exact `plugin/read`, against the
/// provider's own exports. Without a native export (Cursor) a list is neither confirmed
/// nor refuted, so it is never counted as supported.
async fn plugin_legs(
    journey: &Journey,
    plugins: &RpcReply,
    management: &RpcReply,
    native_before: Option<&[NativePluginRow]>,
    checks: &mut Map<String, Value>,
) -> Result<(), String> {
    let marketplaces = journey.native_marketplaces("before").await?;
    let (Some(native_before), Some(marketplaces)) = (native_before, marketplaces) else {
        let status = match &plugins.error {
            Some(error) => CapabilityStatus::ExplicitUnsupported {
                message: error.message.clone(),
            },
            None => CapabilityStatus::NoNativeReference,
        };
        checks.insert("plugins".to_owned(), json_value(&status)?);
        if management.error.is_none() {
            return Err("management write was accepted".to_owned());
        }
        checks.insert("managementRejected".to_owned(), Value::Bool(true));
        return Ok(());
    };
    let plugin_check: PluginCatalogueCheck =
        check_plugin_catalogue(plugins, native_before, &marketplaces)?;
    checks.insert("plugins".to_owned(), json_value(&plugin_check)?);
    let native_after = journey
        .native_plugin_rows("after")
        .await?
        .unwrap_or_default();
    check_management_rejected(management, native_before, &native_after)?;
    checks.insert("managementRejected".to_owned(), Value::Bool(true));
    let (CapabilityStatus::Supported, Some(first_id)) =
        (&plugin_check.status, plugin_check.installed_ids.first())
    else {
        return Ok(());
    };
    // Read back the exact selector this connection returned for the first plugin.
    let native = native_before
        .iter()
        .find(|row| &row.id == first_id)
        .ok_or("listed plugin vanished from the native export")?;
    let view: PluginListView =
        serde_json::from_value(plugins.result.clone().unwrap_or_default())
            .map_err(|error| format!("plugin/list result does not decode: {error}"))?;
    let (marketplace_path, plugin_name) = view
        .marketplaces
        .iter()
        .flat_map(|marketplace| {
            marketplace
                .plugins
                .iter()
                .map(move |plugin| (marketplace, plugin))
        })
        .find(|(_, plugin)| plugin.id == native.id)
        .map(|(marketplace, plugin)| (marketplace.path.clone(), plugin.name.clone()))
        .ok_or("listed plugin has no selector")?;
    let replies = journey
        .requests(
            "plugin-read",
            &[(
                "plugin/read",
                json!({"marketplacePath": marketplace_path, "pluginName": plugin_name}),
            )],
        )
        .await?;
    let [detail] = replies.as_slice() else {
        return Err("plugin/read returned a different reply count".to_owned());
    };
    let facts = native_plugin_facts(native)?;
    write_private(
        &journey.output.join("detail-native-facts.json"),
        &serde_json::to_vec_pretty(&facts).map_err(|error| error.to_string())?,
    )?;
    checks.insert(
        "pluginDetail".to_owned(),
        json_value(&check_plugin_detail(detail, native, &facts)?)?,
    );
    Ok(())
}

/// The real installed setup beside the fixture: one enabled plugin's skill by path.
fn installed_sample(
    skills: &RpcReply,
    workspace: &Path,
    status: &CapabilityStatus,
    native_rows: Option<&[NativePluginRow]>,
) -> Result<Value, String> {
    let Some(native_rows) = native_rows else {
        return Ok(json!("noNativeReference"));
    };
    if *status != CapabilityStatus::Supported {
        return Ok(json!("skillsUnsupported"));
    }
    for row in native_rows.iter().filter(|row| row.enabled) {
        let files = plugin_skill_files(row);
        if !files.is_empty() {
            let path = check_installed_plugin_skill_sample(skills, workspace, row, &files)?;
            return Ok(json!({"pluginId": row.id, "skillPath": path}));
        }
    }
    Err("setup gap: no enabled installed plugin with skills to sample".to_owned())
}

/// J1: skill and plugin discovery, wrong context, management rejection, and the
/// installed client's `$` and `@` popups.
pub(crate) async fn catalogue_journey(journey: &Journey) -> Result<JourneyResult, String> {
    let plan = &journey.target.plan;
    let fixture = &journey.target.fixture;
    let advertised = advertised_command_names(&journey.observe("catalogue", None).await?)?;
    let native_before = journey.native_plugin_rows("before").await?;
    let [skills, wrong, plugins, management] = four_replies(
        journey
            .requests(
                "catalogue",
                &[
                    (
                        "skills/list",
                        json!({"cwds": [fixture.workspace], "forceReload": true}),
                    ),
                    ("skills/list", json!({"cwds": [fixture.empty_workspace]})),
                    ("plugin/list", json!({})),
                    ("plugin/install", json!({"pluginName": ABSENT_PLUGIN})),
                ],
            )
            .await?,
    )?;
    let authority = NativeSkillAuthority {
        provider: plan.endpoint_id,
        advertised_names: advertised.into_iter().collect(),
        home_skill_roots: plan.native_home_skill_roots.clone(),
        workspace_skill_dirs: plan.native_workspace_skill_dirs.clone(),
        home_command_roots: plan.native_home_command_roots.clone(),
        workspace_command_dirs: plan.native_workspace_command_dirs.clone(),
        project_root_marker: plan.native_project_root_marker.clone(),
        // The active folder the provider loads, under the namespace it advertises.
        plugin_namespaces: native_before
            .iter()
            .flatten()
            .filter_map(|row| Some((row.active_root().to_owned(), plugin_namespace(row)?)))
            .collect(),
        // The exact declared skill files of each installed plugin, not its whole folder.
        plugin_skill_sources: native_before
            .iter()
            .flatten()
            .flat_map(plugin_skill_files)
            .collect(),
    };
    let skill_check = check_skill_catalogue(&skills, fixture, &authority)?;
    let mut checks = Map::new();
    checks.insert(
        "wrongContext".to_owned(),
        json_value(&check_wrong_context_catalogue(&wrong, fixture, &authority)?)?,
    );
    checks.insert(
        "installedPluginSkillSample".to_owned(),
        installed_sample(
            &skills,
            &fixture.workspace,
            &skill_check.status,
            native_before.as_deref(),
        )?,
    );
    checks.insert("skills".to_owned(), json_value(&skill_check)?);
    plugin_legs(
        journey,
        &plugins,
        &management,
        native_before.as_deref(),
        &mut checks,
    )
    .await?;
    let visible_name = fixture
        .visible_file
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .ok_or("visible fixture file has no name")?;
    journey.drive_client("catalogue-picker", |client| {
        client.wait_for_ready_composer(COMPOSER_READY, STARTUP, COMPOSER_SETTLE)?;
        let mark = client.mark();
        client.type_text(&format!("${}", name_fragment(&fixture.skill_name)))?;
        let picker = client.wait_for(PICKER_ROW, mark, PICKER_SECONDS)?;
        write_private(&journey.output.join("picker.txt"), picker.as_bytes())?;
        client.send(ESCAPE)?;
        client.send(CLEAR_LINE)?;
        let mark = client.mark();
        client.type_text(&format!(
            "@{}",
            visible_name.get(..12).unwrap_or(&visible_name)
        ))?;
        let popup = client.wait_for(&visible_name, mark, PICKER_SECONDS)?;
        write_private(&journey.output.join("at-file-popup.txt"), popup.as_bytes())
    })?;
    checks.insert("pickerShowedFixture".to_owned(), Value::Bool(true));
    checks.insert("atPopupShowedFile".to_owned(), Value::Bool(true));
    Ok(JourneyResult {
        journey: "J1",
        checks,
        limits: vec![
            "Plugin rows compare with the provider's read-only export: discovery only.",
            "Per-kind Partial/NotObserved detail variants are product-test cases; this observes the real installed plugins as they are.",
            "Hook events compare by name, case and separators aside; semantic mapping to HookEventName is not judged here.",
            "Per-skill enablement inside plugin/read has native overrides not qualified here, so it is not asserted.",
            "Disabled facts use the provider's own frontmatter spellings; settings-file overrides are unverified.",
        ],
    })
}

/// Type the case's input in the real client; return the outcome label and its Text.
fn drive_selection(
    journey: &Journey,
    client: &mut crate::installed_client_terminal::InstalledClientTerminal,
    case: SelectionCase,
    arguments: &str,
) -> Result<(Option<&'static str>, String), String> {
    let fixture = &journey.target.fixture;
    if case == SelectionCase::SelectedSteer {
        client.type_and_submit(STEER_BASE_PROMPT, PICKER_SECONDS)?;
    }
    let prefix = if case == SelectionCase::Nonleading {
        "Please apply "
    } else {
        ""
    };
    // Each Tab selection inserts its own trailing separator (Codex 0.163
    // `chat_composer.rs` insert_selected_mention → advance_past_completion_separator,
    // lines 2772-2820 and 2393-2425), so the driver types no space of its own after a
    // selection. The expected text keeps exactly one space between parts.
    journey.select_skill(client, prefix, &fixture.skill_name, PICKER_ROW)?;
    let mut text = format!("{prefix}${}", fixture.skill_name);
    if case == SelectionCase::TwoSkills {
        journey.select_skill(client, "", &fixture.second_skill_name, SECOND_PICKER_ROW)?;
        text.push_str(&format!(" ${}", fixture.second_skill_name));
    }
    text.push_str(&format!(" {arguments}"));
    let mark = client.type_and_submit(arguments, PICKER_SECONDS)?;
    if case == SelectionCase::Leading {
        client.wait_for(&fixture.canary, mark, REPLY)?;
        return Ok((Some("effect"), text));
    }
    // A rejected steer is judged by its error here; whether the ordinary turn completed
    // is judged from public provider events, because Codex 0.163 stops drawing the
    // ordinary turn after a failed steer.
    let outcome = client.wait_for_effect_or_error(&fixture.canary, mark, REJECTION);
    Ok((outcome, text))
}

/// The selected-steer case's ordinary turn, observed from the pre-drive cursor until a
/// turn ends or the reply bound passes. Every page is kept under its own label.
async fn observe_until_turn_ends(
    journey: &Journey,
    cursor: ObservationCursor,
) -> Result<ProviderObservation, String> {
    let deadline = Instant::now() + REPLY;
    let mut attempt = 0_u32;
    loop {
        let observed = journey
            .observe(&format!("after-{attempt}"), Some(cursor))
            .await?;
        if any_turn_ended(&observed) || Instant::now() >= deadline {
            return Ok(observed);
        }
        attempt += 1;
    }
}

/// J2: installed-Codex `$` selection; `leading` is the positive case, the others are
/// explicit rejections that must send nothing.
pub(crate) async fn selection_journey(
    journey: &Journey,
    case: SelectionCase,
    arguments: &str,
) -> Result<JourneyResult, String> {
    let fixture = &journey.target.fixture;
    let label = format!("select-{}", case.as_str());
    let retained_before = journey.observe("before", None).await?;
    if case == SelectionCase::Leading {
        require_fresh_session(&retained_before, fixture)?;
    }
    let cursor = retained_before.cursor();
    let before = journey.operations("before").await?;
    // Rejection cases also read the delivery queue: a queued selection would add no
    // prompt or turn while it waits.
    let queue_before = match case {
        SelectionCase::Leading => None,
        _ => Some(journey.queue("queue-before").await?),
    };
    let (outcome, text) = journey.drive_client(&label, |client| {
        client.wait_for_ready_composer(COMPOSER_READY, STARTUP, COMPOSER_SETTLE)?;
        drive_selection(journey, client, case, arguments)
    })?;
    let expected_skills = if case == SelectionCase::TwoSkills {
        vec![
            fixture.skill_name.as_str(),
            fixture.second_skill_name.as_str(),
        ]
    } else {
        vec![fixture.skill_name.as_str()]
    };
    let recorded = check_recorded_selection(
        &read_session_recorder(&journey.recorder(&label))?,
        fixture,
        &text,
        &expected_skills,
        if case == SelectionCase::SelectedSteer {
            2
        } else {
            1
        },
    )?;
    let after = if case == SelectionCase::SelectedSteer {
        observe_until_turn_ends(journey, cursor).await?
    } else {
        journey.observe("after", Some(cursor)).await?
    };
    let operations = journey.operations("after").await?;
    let mut checks = Map::new();
    checks.insert("case".to_owned(), json!(case.as_str()));
    checks.insert("clientInput".to_owned(), json_value(&recorded)?);
    if case == SelectionCase::SelectedSteer {
        // Observation only: the client may stop drawing the ordinary turn.
        let shown = read_bounded_bytes(&journey.capture(&label), CAPTURE_MAX_BYTES as u64)
            .map(|raw| terminal_text(&raw).contains(STEER_BASE_REPLY))?;
        checks.insert("clientShowedOrdinaryReply".to_owned(), Value::Bool(shown));
    }
    if case == SelectionCase::Leading {
        let selected =
            check_native_invocation(&after, fixture, arguments, &[text], &before, &operations)?;
        checks.insert("selected".to_owned(), json_value(&selected)?);
    } else {
        if case == SelectionCase::SelectedSteer && selected_steer_raced(&before, &operations) {
            return Err("harness race: the ordinary turn ended before the steer; rerun".to_owned());
        }
        let queue = QueueObservation {
            before: queue_before.unwrap_or_default(),
            after: journey.queue("queue-after").await?,
        };
        let rejected = check_rejected_input(
            &after,
            fixture,
            &before,
            &operations,
            &queue,
            outcome == Some("error"),
            usize::from(case == SelectionCase::SelectedSteer),
        )?;
        checks.insert("rejected".to_owned(), json_value(&rejected)?);
        checks.insert("deliveryQueue".to_owned(), json_value(&queue)?);
        if case == SelectionCase::SelectedSteer {
            let ordinary = check_ordinary_completion(&after, STEER_BASE_REPLY)?;
            checks.insert("ordinaryCompletion".to_owned(), json_value(&ordinary)?);
        }
    }
    Ok(JourneyResult {
        journey: "J2",
        checks,
        limits: vec![
            "Client-visible rejection wording is recorded, not asserted.",
            "After a failed steer the Codex 0.163 client may stop drawing the ordinary turn; its completion is judged from public provider events.",
        ],
    })
}

/// J3: catalogue replacement, Router `/` invocation and installed-Codex picker refusal.
pub(crate) async fn commands_journey(
    journey: &Journey,
    arguments: &str,
    reload_probe: bool,
) -> Result<JourneyResult, String> {
    let fixture = &journey.target.fixture;
    let observed = journey.observe("catalogues", None).await?;
    require_fresh_session(&observed, fixture)?;
    let catalogues = check_command_catalogues(&observed, fixture)?;
    let before = journey.operations("before").await?;
    let text = format!("/{} {arguments}", fixture.skill_name);
    let prompt_exit = journey.router_prompt("router-prompt", &text, REPLY).await?;
    let after = journey.observe("after", Some(observed.cursor())).await?;
    let invoked = journey.operations("invoked").await?;
    let invocation =
        check_native_invocation(&after, fixture, arguments, &[text], &before, &invoked)?;
    let screen = journey.drive_client("picker-refusal", |client| {
        client.wait_for_ready_composer(COMPOSER_READY, STARTUP, COMPOSER_SETTLE)?;
        let mark = client.type_and_submit(
            &format!("/{} refusal-probe", fixture.skill_name),
            PICKER_SECONDS,
        )?;
        client.wait_for("Unrecognized command", mark, PICKER_SECONDS)
    })?;
    check_picker_refusal(&screen, &fixture.skill_name)?;
    if !new_operations(&invoked, &journey.operations("refused").await?).is_empty() {
        return Err("local picker refusal still reached a Router operation".to_owned());
    }
    let mut checks = Map::new();
    checks.insert("routerPromptExit".to_owned(), json!(prompt_exit));
    checks.insert("catalogues".to_owned(), json_value(&catalogues)?);
    checks.insert("invocation".to_owned(), json_value(&invocation)?);
    checks.insert("pickerRefusal".to_owned(), Value::Bool(true));
    let mut limits = vec![
        "Codex's slash menu cannot list provider commands; that is a client limit.",
        "Identical successive command lists leave replacement unobserved, not passed.",
    ];
    if reload_probe {
        let probe = reload_after_fixture_removal(journey, REPLY).await?;
        checks.insert("reloadProbe".to_owned(), json_value(&probe)?);
        limits.push(
            "The reload probe removes only the second fixture skill and restores its exact bytes; a list without the removal leaves replacement unobserved.",
        );
    }
    Ok(JourneyResult {
        journey: "J3",
        checks,
        limits,
    })
}

fn history_thread(reply: &RpcReply) -> Result<HistoryThread, String> {
    if reply.error.is_some() {
        return Err("history read failed; raw reply retained".to_owned());
    }
    let thread = reply
        .result
        .as_ref()
        .and_then(|result| result.get("thread"))
        .cloned()
        .ok_or("history reply has no thread")?;
    serde_json::from_value(thread)
        .map_err(|error| format!("history thread does not decode: {error}"))
}

/// J4: read/resume canonical history, display operation counts, client reopen, and the
/// merged ACP metadata oracle. The ordinary-prose control is the selected-steer case's
/// STEER_BASE_OK turn: its live capture is that drive's PTY, its reopening is this one.
pub(crate) async fn history_journey(
    journey: &Journey,
    arguments: &str,
    unloaded_resume: bool,
    ordinary_live_capture: &Path,
) -> Result<JourneyResult, String> {
    let plan = &journey.target.plan;
    let fixture = &journey.target.fixture;
    let live_capture = read_bounded_bytes(ordinary_live_capture, CAPTURE_MAX_BYTES as u64)?;
    let retained = journey.observe("before", None).await?;
    let before = journey.operations("before").await?;
    let replies = journey
        .requests(
            "history",
            &[
                (
                    "thread/read",
                    json!({"threadId": plan.session_id, "includeTurns": true}),
                ),
                (
                    "thread/resume",
                    json!({"threadId": plan.session_id, "excludeTurns": false}),
                ),
            ],
        )
        .await?;
    let threads = replies
        .iter()
        .map(history_thread)
        .collect::<Result<Vec<_>, _>>()?;
    journey.drive_client("history-reopen", |client| {
        client.wait_for(&fixture.canary, 0, STARTUP)?;
        client.wait_for(STEER_BASE_REPLY, 0, STARTUP).map(drop)
    })?;
    let display = check_display_operations(
        &before,
        &journey.operations("after").await?,
        &journey.observe("display", Some(retained.cursor())).await?,
        unloaded_resume,
    )?;
    let history = check_selected_history(&threads, &plan.session_id, fixture, arguments)?;
    let reopened =
        read_bounded_bytes(&journey.capture("history-reopen"), CAPTURE_MAX_BYTES as u64)?;
    let metadata = check_metadata_observation(
        &retained,
        &threads,
        &terminal_text(&live_capture),
        &terminal_text(&reopened),
        STEER_BASE_REPLY,
    )?;
    let mut checks = Map::new();
    checks.insert("history".to_owned(), json_value(&history)?);
    checks.insert("display".to_owned(), json_value(&display)?);
    checks.insert("metadata".to_owned(), json_value(&metadata)?);
    checks.insert(
        "expectedReply".to_owned(),
        json!(expected_native_reply(&fixture.canary, arguments)),
    );
    Ok(JourneyResult {
        journey: "J4",
        checks,
        limits: vec![
            "Router operation rows and public events, not native-provider counters.",
            "Usage stays unobserved when the provider emits none (metadata usage_limit).",
        ],
    })
}
