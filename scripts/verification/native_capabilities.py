"""Drive and read back the native-capabilities journeys J1-J4 on a declared candidate.

Run from the repository root (journey: `.agents/skills/verify-codex-router/references/
native-capabilities.md`):

  materialize --workspace W --empty-workspace E --record FIXTURE.json
      Create owned workspaces with the controlled Claude project skills and fresh canaries.
  catalogue PLAN --output DIR       J1 discovery, wrong context, plugins, `$` and `@` popups
  select PLAN --case CASE --arguments TEXT --output DIR
      J2 installed-Codex `$` selection. CASE: leading (positive), nonleading,
      two-skills, selected-steer (explicit rejections)
  commands PLAN --arguments TEXT --output DIR
      J3 catalogue replacement, Router `/` invocation, installed-Codex picker refusal
  history PLAN --arguments TEXT --output DIR --ordinary-live-capture PTY
          [--unloaded-resume]
      J4 read/resume canonical history, display operation counts, client reopen and
      the unchanged ACP metadata oracle (ordinary control: the selected-steer turn)

PLAN is a NativeCapabilitiesPlan JSON naming the `owned_runtime` RuntimePlan whose
candidate is running; qualification reuses that lifecycle's strict signature, live
identity and native-binding checks and fails before any drive. Each run writes raw
evidence to a fresh private DIR and prints a typed result. Exit 1 means the oracle
rejected the evidence or the run broke: read DIR/failure.txt. The driver never
creates Sessions, logs in, edits installations or stops a Host; the operator owns
those. Router operation rows are Router records, never native-provider counters.
"""

import argparse
import json
import subprocess
import sys
import typing as t
from pathlib import Path

from pydantic import JsonValue

from scripts.verification.acp_metadata_observation import (
    CodexThread,
    check_metadata_observation,
    save_raw,
    terminal_text,
)
from scripts.verification.native_capabilities_catalogue_oracle import (
    check_installed_plugin_skill_sample,
    check_management_rejected,
    check_plugin_catalogue,
    check_plugin_detail,
    check_skill_catalogue,
    check_wrong_context_catalogue,
)
from scripts.verification.native_capabilities_fixture import materialize_fixture
from scripts.verification.native_capabilities_journey import Journey, JourneyResult
from scripts.verification.native_capabilities_models import (
    HistoryThread,
    NativeCapabilitiesPlan,
    NativePluginRow,
    NativeSkillAuthority,
    PluginCatalogueCheck,
    PluginListView,
    RpcReply,
)
from scripts.verification.native_capabilities_oracle import (
    advertised_command_names,
    check_command_catalogues,
    check_display_operations,
    check_native_invocation,
    check_picker_refusal,
    check_recorded_selection,
    check_rejected_input,
    check_selected_history,
    expected_native_reply,
    new_operations,
    selected_steer_raced,
)
from scripts.verification.native_capabilities_target import (
    codex_config_digest,
    native_marketplaces,
    native_plugin_facts,
    native_plugin_rows,
    observation_cursor,
    plugin_skill_files,
    read_session_recorder,
)
from scripts.verification.native_capabilities_terminal import (
    ENTER,
    InstalledCodexTerminal,
    TerminalTimeout,
)

COMPOSER_READY = r"›"
PICKER_ROW = r"echoes its arguments"
SECOND_PICKER_ROW = r"must be rejected"
CLIENT_ERROR = r"(?i)\b(error|unsupported|rejected|not supported)\b"
STARTUP_SECONDS = 90.0
REPLY_SECONDS = 180.0
REJECTION_SECONDS = 60.0
# The ordinary turn a selected steer targets; its reply is absent from the prompt.
STEER_BASE_PROMPT = "Reply with exactly three uppercase words joined by underscores: STEER, BASE, OK. Do not use tools."
STEER_BASE_REPLY = "STEER_BASE_OK"

type SelectionCase = t.Literal["leading", "nonleading", "two-skills", "selected-steer"]


def plugin_legs(
    journey: Journey,
    plugins: RpcReply,
    management: RpcReply,
    native_before: list[NativePluginRow] | None,
) -> dict[str, JsonValue]:
    target, output = journey.target, journey.output
    checks: dict[str, JsonValue] = {}
    marketplaces = native_marketplaces(target, output, "before")
    if native_before is None or marketplaces is None:
        # No qualified native plugin reference exists for this provider; a list can be
        # neither confirmed nor refuted, so it is never counted as supported.
        checks["plugins"] = PluginCatalogueCheck(
            status="explicitUnsupported"
            if plugins.error is not None
            else "noNativeReference",
            unsupported_message=plugins.error.message if plugins.error else None,
        ).model_dump()
        checks["managementRejected"] = management.error is not None
        if management.error is None:
            raise ValueError("management write was accepted")
        return checks
    plugin_check = check_plugin_catalogue(plugins, native_before, marketplaces)
    checks["plugins"] = plugin_check.model_dump()
    native_after = native_plugin_rows(target, output, "after")
    check_management_rejected(management, native_before, native_after or [])
    checks["managementRejected"] = True
    if plugin_check.status == "supported" and plugin_check.installed_ids:
        # Read back the exact selector this connection returned for the first plugin.
        native = next(
            row for row in native_before if row.id == plugin_check.installed_ids[0]
        )
        marketplace, summary = next(
            (marketplace, plugin)
            for marketplace in PluginListView.model_validate(
                plugins.result
            ).marketplaces
            for plugin in marketplace.plugins
            if plugin.id == native.id
        )
        (detail,) = journey.requests(
            "plugin-read",
            [
                (
                    "plugin/read",
                    {"marketplacePath": marketplace.path, "pluginName": summary.name},
                )
            ],
        )
        facts = native_plugin_facts(native, output, "detail")
        checks["pluginDetail"] = check_plugin_detail(detail, native, facts)
    return checks


def installed_sample(
    skills: RpcReply,
    workspace: Path,
    status: str,
    native_rows: list[NativePluginRow] | None,
) -> JsonValue:
    """The real installed setup beside the fixture: one enabled plugin's skill by path."""
    if native_rows is None:
        return "noNativeReference"
    if status != "supported":
        return "skillsUnsupported"
    for row in native_rows:
        files = plugin_skill_files(row)
        if row.enabled and files:
            path = check_installed_plugin_skill_sample(skills, workspace, row, files)
            return {"pluginId": row.id, "skillPath": path}
    raise ValueError("setup gap: no enabled installed plugin with skills to sample")


def catalogue_journey(journey: Journey) -> JourneyResult:
    target, output = journey.target, journey.output
    plan, fixture = target.plan, target.fixture
    advertised = advertised_command_names(journey.observe("catalogue", None))
    native_before = native_plugin_rows(target, output, "before")
    skills, wrong, plugins, management = journey.requests(
        "catalogue",
        [
            ("skills/list", {"cwds": [str(fixture.workspace)], "forceReload": True}),
            ("skills/list", {"cwds": [str(fixture.empty_workspace)]}),
            ("plugin/list", {}),
            # A plugin name the provider cannot have, so even a wrongly dispatched
            # install has nothing to install.
            ("plugin/install", {"pluginName": "router-verifier-absent-plugin"}),
        ],
    )
    authority = NativeSkillAuthority(
        advertised_names=frozenset(advertised),
        home_roots=tuple(plan.native_home_skill_roots),
        workspace_dirs=tuple(plan.native_workspace_skill_dirs),
        project_root_marker=plan.native_project_root_marker,
        # Namespaces come from the provider's own plugin export (`name@marketplace`).
        plugin_namespaces=tuple(
            (Path(row.installPath), row.id.rpartition("@")[0])
            for row in native_before or []
        ),
        disabled_markers=tuple(plan.native_disabled_markers.items()),
    )
    skill_check = check_skill_catalogue(skills, fixture, authority)
    checks: dict[str, JsonValue] = {
        "skills": skill_check.model_dump(),
        "wrongContext": check_wrong_context_catalogue(
            wrong, fixture, authority
        ).model_dump(),
        "installedPluginSkillSample": installed_sample(
            skills, fixture.workspace, skill_check.status, native_before
        ),
        **plugin_legs(journey, plugins, management, native_before),
    }
    digest_before = codex_config_digest()
    with journey.terminal("catalogue-picker") as codex:
        _ = codex.wait_for(COMPOSER_READY, 0, STARTUP_SECONDS)
        mark = codex.mark()
        codex.type_text("$" + fixture.skill_name[:-4])
        save_raw(output / "picker.txt", codex.wait_for(PICKER_ROW, mark, 30))
        codex.send(b"\x1b")  # close the popup
        codex.send(b"\x15")  # clear the composer line
        mark = codex.mark()
        codex.type_text("@" + fixture.visible_file.name[:12])
        save_raw(
            output / "at-file-popup.txt",
            codex.wait_for(fixture.visible_file.name, mark, 30),
        )
    if codex_config_digest() != digest_before:
        raise ValueError("global Codex configuration changed during the drive")
    checks["pickerShowedFixture"] = True
    checks["atPopupShowedFile"] = True
    return JourneyResult(
        journey="J1",
        checks=checks,
        limits=[
            "Plugin rows compare with the provider's read-only export: discovery only.",
            (
                "Per-kind Partial/NotObserved detail variants are product-test cases; "
                "this observes the real installed plugins as they are."
            ),
            (
                "Hook events compare by name, case and separators aside; semantic "
                "mapping to HookEventName is not judged here."
            ),
        ],
    )


def drive_selection(
    journey: Journey, codex: InstalledCodexTerminal, case: SelectionCase, arguments: str
) -> tuple[str | None, str]:
    """Type the case's input in the real client; return the outcome label and its Text."""
    fixture = journey.target.fixture
    if case == "selected-steer":
        codex.type_text(STEER_BASE_PROMPT)
        codex.send(ENTER)
    prefix = "Please apply " if case == "nonleading" else ""
    journey.select_skill(codex, prefix, fixture.skill_name, PICKER_ROW)
    text = f"{prefix}${fixture.skill_name}"
    if case == "two-skills":
        codex.type_text(" ")
        journey.select_skill(codex, "", fixture.second_skill_name, SECOND_PICKER_ROW)
        text += f" ${fixture.second_skill_name}"
    codex.type_text(" " + arguments)
    text += " " + arguments
    mark = codex.mark()
    codex.send(ENTER)
    if case == "leading":
        _ = codex.wait_for(fixture.canary, mark, REPLY_SECONDS)
        return "effect", text
    outcome = codex.wait_for_any(
        {"effect": fixture.canary, "error": CLIENT_ERROR}, mark, REJECTION_SECONDS
    )
    if case == "selected-steer":
        _ = codex.wait_for(STEER_BASE_REPLY, 0, REPLY_SECONDS)
    return outcome, text


def selection_journey(
    journey: Journey, case: SelectionCase, arguments: str
) -> JourneyResult:
    fixture = journey.target.fixture
    cursor = observation_cursor(journey.observe("before", None))
    before = journey.operations("before")
    digest_before = codex_config_digest()
    with journey.terminal(f"select-{case}") as codex:
        _ = codex.wait_for(COMPOSER_READY, 0, STARTUP_SECONDS)
        outcome, text = drive_selection(journey, codex, case, arguments)
    if codex_config_digest() != digest_before:
        raise ValueError("global Codex configuration changed during the drive")
    recorded = check_recorded_selection(
        read_session_recorder(journey.recorder(f"select-{case}")),
        fixture,
        text,
        [fixture.skill_name, fixture.second_skill_name]
        if case == "two-skills"
        else [fixture.skill_name],
        expected_turns=2 if case == "selected-steer" else 1,
    )
    after = journey.observe("after", cursor)
    operations = journey.operations("after")
    checks: dict[str, JsonValue] = {"case": case, "clientInput": recorded.model_dump()}
    if case == "leading":
        selected = check_native_invocation(
            after, fixture, arguments, [text], before, operations
        )
        checks["selected"] = selected.model_dump()
    else:
        if case == "selected-steer" and selected_steer_raced(before, operations):
            raise ValueError(
                "harness race: the ordinary turn ended before the steer; rerun"
            )
        rejected = check_rejected_input(
            after,
            fixture,
            before,
            operations,
            client_error_seen=outcome == "error",
            ordinary_prompts=1 if case == "selected-steer" else 0,
        )
        checks["rejected"] = rejected.model_dump()
    return JourneyResult(
        journey="J2",
        checks=checks,
        limits=["Client-visible rejection wording is recorded, not asserted."],
    )


def commands_journey(journey: Journey, arguments: str) -> JourneyResult:
    target, output = journey.target, journey.output
    plan, fixture = target.plan, target.fixture
    observed = journey.observe("catalogues", None)
    catalogues = check_command_catalogues(observed, fixture)
    before = journey.operations("before")
    text = f"/{fixture.skill_name} {arguments}"
    prompt_file = output / "router-prompt.txt"
    save_raw(prompt_file, text)
    completed = subprocess.run(
        [
            str(target.collaboration_cli),
            "conversation",
            "prompt",
            "--endpoint",
            plan.endpoint_id,
            "--session",
            plan.session_id,
            "--text-file",
            str(prompt_file),
            "--timeout-seconds",
            str(int(REPLY_SECONDS)),
            "--service-directory",
            str(target.service_directory),
            "--json",
        ],
        capture_output=True,
        text=True,
        timeout=REPLY_SECONDS + 30,
        check=False,
    )
    save_raw(output / "router-prompt.stdout", completed.stdout)
    save_raw(output / "router-prompt.stderr", completed.stderr)
    after = journey.observe("after", observation_cursor(observed))
    invoked = journey.operations("invoked")
    invocation = check_native_invocation(
        after, fixture, arguments, [text], before, invoked
    )
    with journey.terminal("picker-refusal") as codex:
        _ = codex.wait_for(COMPOSER_READY, 0, STARTUP_SECONDS)
        mark = codex.mark()
        codex.type_text(f"/{fixture.skill_name} refusal-probe")
        codex.send(ENTER)
        screen = codex.wait_for("Unrecognized command", mark, 30)
    check_picker_refusal(screen, fixture.skill_name)
    if new_operations(invoked, journey.operations("refused")):
        raise ValueError("local picker refusal still reached a Router operation")
    return JourneyResult(
        journey="J3",
        checks={
            "routerPromptExit": completed.returncode,
            "catalogues": catalogues.model_dump(),
            "invocation": invocation.model_dump(),
            "pickerRefusal": True,
        },
        limits=[
            "Codex's slash menu cannot list provider commands; that is a client limit."
        ],
    )


def history_journey(
    journey: Journey,
    arguments: str,
    unloaded_resume: bool,
    ordinary_live_capture: Path,
) -> JourneyResult:
    """J4. The ordinary-prose control is the selected-steer case's STEER_BASE_OK turn:
    its live capture is that drive's PTY and its reopened rendering is this one."""
    plan, fixture = journey.target.plan, journey.target.fixture
    retained = journey.observe("before", None)
    cursor = observation_cursor(retained)
    before = journey.operations("before")
    read, resume = journey.requests(
        "history",
        [
            ("thread/read", {"threadId": plan.session_id, "includeTurns": True}),
            ("thread/resume", {"threadId": plan.session_id, "excludeTurns": False}),
        ],
    )
    threads: list[dict[str, JsonValue]] = []
    for reply in (read, resume):
        if reply.error is not None:
            raise ValueError("history read failed; raw reply retained")
        threads.append(
            t.cast(
                dict[str, JsonValue],
                t.cast(dict[str, JsonValue], reply.result)["thread"],
            )
        )
    with journey.terminal("history-reopen") as codex:
        _ = codex.wait_for(fixture.canary, 0, STARTUP_SECONDS)
        _ = codex.wait_for(STEER_BASE_REPLY, 0, STARTUP_SECONDS)
    display = check_display_operations(
        before,
        journey.operations("after"),
        journey.observe("display", cursor),
        unloaded_resume=unloaded_resume,
    )
    history = check_selected_history(
        [HistoryThread.model_validate(thread) for thread in threads],
        plan.session_id,
        fixture,
        arguments,
    )
    # The merged metadata repair's own oracle, unchanged, on the same Session.
    metadata = check_metadata_observation(
        retained,
        [CodexThread.model_validate(thread) for thread in threads],
        terminal_text(ordinary_live_capture.read_bytes()),
        terminal_text((journey.output / "history-reopen.pty").read_bytes()),
        STEER_BASE_REPLY,
        False,
    )
    return JourneyResult(
        journey="J4",
        checks={
            "history": history.model_dump(),
            "display": display.model_dump(),
            "metadata": metadata.model_dump(),
            "expectedReply": expected_native_reply(fixture.canary, arguments),
        },
        limits=[
            "Router operation rows and public events, not native-provider counters.",
            "Usage stays unobserved when the provider emits none (metadata usage_limit).",
        ],
    )


class Arguments(argparse.Namespace):
    command: str = ""
    plan: Path = Path()
    output: Path = Path()
    workspace: Path = Path()
    empty_workspace: Path = Path()
    record: Path = Path()
    case: SelectionCase = "leading"
    arguments: str = ""
    unloaded_resume: bool = False
    ordinary_live_capture: Path = Path()


def parser() -> argparse.ArgumentParser:
    root = argparse.ArgumentParser(
        description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter
    )
    commands = root.add_subparsers(dest="command", required=True)
    materialize = commands.add_parser(
        "materialize", help="create owned fixture workspaces"
    )
    _ = materialize.add_argument("--workspace", type=Path, required=True)
    _ = materialize.add_argument("--empty-workspace", type=Path, required=True)
    _ = materialize.add_argument("--record", type=Path, required=True)
    for name, help_text in [
        ("catalogue", "J1 discovery"),
        ("select", "J2 installed-client selection"),
        ("commands", "J3 command lists and invocation"),
        ("history", "J4 history and display operations"),
    ]:
        journey = commands.add_parser(name, help=help_text)
        _ = journey.add_argument("plan", type=Path)
        _ = journey.add_argument("--output", type=Path, required=True)
        if name != "catalogue":
            _ = journey.add_argument("--arguments", required=True)
        if name == "select":
            _ = journey.add_argument(
                "--case",
                choices=["leading", "nonleading", "two-skills", "selected-steer"],
                required=True,
            )
        if name == "history":
            _ = journey.add_argument("--unloaded-resume", action="store_true")
            _ = journey.add_argument(
                "--ordinary-live-capture",
                type=Path,
                required=True,
                help="the select --case selected-steer run's select-selected-steer.pty",
            )
    return root


def main() -> None:
    arguments = parser().parse_args(namespace=Arguments())
    if arguments.command == "materialize":
        fixture = materialize_fixture(
            arguments.workspace, arguments.empty_workspace, arguments.record
        )
        print(fixture.model_dump_json(indent=2))
        return
    plan = NativeCapabilitiesPlan.model_validate_json(arguments.plan.read_bytes())
    output = arguments.output
    try:
        journey = Journey(plan, output)
        match arguments.command:
            case "catalogue":
                result = catalogue_journey(journey)
            case "select":
                result = selection_journey(journey, arguments.case, arguments.arguments)
            case "commands":
                result = commands_journey(journey, arguments.arguments)
            case _:
                result = history_journey(
                    journey,
                    arguments.arguments,
                    arguments.unloaded_resume,
                    arguments.ordinary_live_capture,
                )
    except (
        ValueError,
        RuntimeError,
        OSError,
        TerminalTimeout,
        subprocess.SubprocessError,
    ) as error:
        if output.is_dir():
            save_raw(output / "failure.txt", f"{type(error).__name__}: {error}\n")
        print(f"{type(error).__name__}: {error}", file=sys.stderr)
        sys.exit(1)
    save_raw(output / "result.json", result.model_dump_json(indent=2))
    print(json.dumps(result.model_dump(), indent=2))


if __name__ == "__main__":
    main()
