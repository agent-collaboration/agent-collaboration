"""Current local Worker entrypoints and explicit legacy retirements.

The conformance bundle uses dev. Production Worker builds and measurements are
outside this runner; their artifact profile is worker-release (decision 515).
"""

import hashlib
import sqlite3
import typing as t
from contextlib import closing
from dataclasses import dataclass
from pathlib import Path
from uuid import UUID

from pydantic import BaseModel, ConfigDict

DEFAULT_WORKERD_MODES = ("composed-transactions", "composed-numeric")
RETIRED_LAUNCHERS = {
    "workerd_lifecycle": "Retired by decisions 508/513; choose a current scenario explicitly.",
    "native_smoke": "Retired by decision 508; name current native integration targets.",
    "staging_driver": "Retired by decision 508; OB7 needs a new declared staging entry.",
}
RETIRED_NATIVE_TARGETS = {
    "local_storage": "Retired with the legacy SqlTx/local runtime by decision 508; no equivalent proof is inferred.",
    "local_conformance": "Retired with the legacy local runtime by decision 508; name a current integration target.",
}
RETIRED_WORKERD_SCENARIOS = {
    "conformance": "Retired legacy aggregate (508); the current composed modes do not prove all former groups.",
    "dag-edge": "Retired legacy Copy-to-Registry control (508); caller-refusal proof is still owed.",
    "access-profile": "Retired launcher (508); explicitly select access-carryover for its narrower current four groups.",
    "access-zero-age": "Retired launcher (508); explicitly select access-carryover for its current four groups.",
    "access-workflow": "Retired legacy workflow (508); access-carryover is not the former outer/inner log-layer proof.",
    "default": "Retired legacy-shell default session (513); select session-capabilities and session-failures for the declared normal-path replacements.",
    "holder-controls": "Retired legacy-shell holder controls (513); select holder-lifecycle with its explicit fifth-group primitive/wire split.",
    "panic-inside-transaction": "Retired G18 SqlTx hook (513); use the separately commissioned current-boundary proof; hosted panic/V3d remains separate.",
}


@dataclass(frozen=True)
class WorkerdScenario:
    example: str
    marker: str
    argument_order: t.Literal["mode-first", "runtime-only", "mode-last"] = "mode-first"
    minimum_reaped_processes: int = 1


WORKERD_SCENARIOS = {
    "mailbox-deadline": WorkerdScenario(
        "session_scope_workerd",
        "MAILBOX_DEADLINE_RESULT 1 passed; 0 failed; 0 ignored (shared normal session, offline recipient, real alarm, newer status and released payload; no physical restart claim)",
        "mode-last",
    ),
    "installation-directory": WorkerdScenario(
        "session_scope_workerd",
        "INSTALLATION_DIRECTORY_RESULT 3 groups passed; 0 failed; 0 ignored (shared normal-session multi-page ordering and cursor refusals, immutable revoked bindings plus submit refusal)",
        "mode-last",
    ),
    "delivery-config-refusals": WorkerdScenario(
        "session_scope_workerd",
        "DELIVERY_TEMPLATE_REFUSALS_RESULT 2 passed; 0 failed; 0 ignored (real Worker missing relay field and malformed peer refusal)",
        "mode-last",
    ),
    "claim-archived": WorkerdScenario(
        "session_scope_workerd",
        "CLAIM_ARCHIVED_RESULT 1 passed; 0 failed; 0 ignored (shared Native/Worker scenario; actual Archived unretired imported seed, normal Claim exact board_archived refusal and owning Fleet reservation absent before/after; no D1b HTTP import or physical restart claim)",
        "mode-last",
        1,
    ),
    "archive-reports": WorkerdScenario(
        "session_scope_workerd",
        "ARCHIVE_REPORT_RESULT 1 passed; 0 failed; 0 ignored (same retained archive-report oracle as Native; actual normal Worker sessions, owning Fleet clock, exact first-report/replay/board/workspace/feed/ledger observations; no reservation-race or hosted claim)",
        "mode-last",
        1,
    ),
    "imported-claim": WorkerdScenario(
        "session_scope_workerd",
        "IMPORTED_CLAIM_RESULT 2 passed; 0 failed; 0 ignored (conformance Open import/unheld/seed on actual composed stores; normal owner retirement refusal with unchanged seed, normal exact Claim/replay/fresh AlreadyHeld, Fleet Directory and Copy fence; no D1b HTTP import, private publication-ack read, Retiring/Retired precedence or physical restart claim)",
        "mode-last",
        1,
    ),
    "claim-retiring": WorkerdScenario(
        "session_scope_workerd",
        "CLAIM_RETIRING_RESULT 1 passed; 0 failed; 0 ignored (real imported seed and normal owner CopyMismatch retirement; cached Register overridden, old Claim NotFound replay retained, fresh Claim exact transfer refusal, normal Directory Unheld; no private publication-ack or physical restart claim)",
        "mode-last",
        1,
    ),
    "claim-retired": WorkerdScenario(
        "session_scope_workerd",
        "CLAIM_RETIRED_RESULT 1 passed; 0 failed; 0 ignored (real imported seed and normal completed owner retirement; Retired outranks cached Register, cached Claim and fresh Claim; no D1b admin import or physical restart claim)",
        "mode-last",
        1,
    ),
    "forward-origin": WorkerdScenario(
        "session_scope_workerd",
        "FORWARD_SESSION_RESULT 1 passed; 0 failed; 0 ignored (selected retained Native oracle through actual normal Worker enrollment/Fleet-create/Register/session/Registry/holder; no capacity, deadline, physical restart or full row19 claim)",
        "mode-last",
        1,
    ),
    "forward-disconnected": WorkerdScenario(
        "session_scope_workerd",
        "FORWARD_SESSION_RESULT 1 passed; 0 failed; 0 ignored (selected retained Native oracle through actual normal Worker enrollment/Fleet-create/Register/session/Registry/holder; no capacity, deadline, physical restart or full row19 claim)",
        "mode-last",
        1,
    ),
    "forward-reconnect": WorkerdScenario(
        "session_scope_workerd",
        "FORWARD_SESSION_RESULT 1 passed; 0 failed; 0 ignored (selected retained Native oracle through actual normal Worker enrollment/Fleet-create/Register/session/Registry/holder; no capacity, deadline, physical restart or full row19 claim)",
        "mode-last",
        1,
    ),
    "forward-capacity": WorkerdScenario(
        "session_scope_workerd",
        "FORWARD_CAPACITY_RESULT 1 passed; 0 failed; 0 ignored (shared retained Native oracle, normal Worker enrollment/Fleet/Register; actual sender Registry-dispatch barrier, reserved sixteenth slot, exact seventeenth Overloaded and terminal capacity reuse; no process restart or full row19 claim)",
        "mode-last",
        1,
    ),
    "forward-deadline": WorkerdScenario(
        "session_scope_workerd",
        "FORWARD_DEADLINE_RESULT 1 passed; 0 failed; 0 ignored (shared retained Native oracle, normal Worker enrollment/Fleet/Register; actual owning sender clock/alarm, original Unanswered and late-reply isolation from fresh identity; no process restart or full row19 claim)",
        "mode-last",
        1,
    ),
    "forward-reopen": WorkerdScenario(
        "session_scope_workerd",
        "FORWARD_REOPEN_RESULT 1 passed; 0 failed; 0 ignored (two actual owned Worker process lifetimes; stopped persistence known-correlation presence/opaque absence, sixteen retained slots before original deadline and sixteen fresh deliveries after expiry; public capacity observation, no private SQL read, DO eviction/hibernation or real Router writer claim)",
        "mode-last",
        2,
    ),
    "composed-copy-suite": WorkerdScenario(
        "composed_scope_workerd",
        "COMPOSED_COPY_SUITE ordered_receipts_staging_fences_rollback_paging_and_physical_restarts passed",
    ),
    "composed-transactions": WorkerdScenario(
        "composed_scope_workerd",
        "COMPOSED_TRANSACTION_CASE sql_and_full_rollbacks_fence_300_row_snapshot_prefixes_and_reopen passed",
        minimum_reaped_processes=2,
    ),
    "composed-numeric": WorkerdScenario(
        "composed_scope_workerd",
        "COMPOSED_NUMERIC_CASE exact_i64_cells_real_identity_snapshot_and_receipt_replay passed",
        minimum_reaped_processes=2,
    ),
    "runtime-controls": WorkerdScenario(
        "composed_scope_workerd",
        "RUNTIME_CONTROLS_CASE real_object_clock_late_storage_rollback_exact_answer_drop_and_owned_dispatch_hold passed",
    ),
    "composed-fleet-conflict": WorkerdScenario(
        "composed_scope_workerd",
        "COMPOSED_FLEET_CONFLICT exact_original_decision_canonical_records_and_head_unchanged passed",
    ),
    "fleet-receipt": WorkerdScenario(
        "step_scope_workerd",
        "STEP_RESULT 10 distinct groups passed; 0 failed; 0 ignored (real DO store and SQLx; test HTTP controls, no production route claim)",
        "runtime-only",
        2,
    ),
    "bootstrap": WorkerdScenario(
        "bootstrap_scope_workerd",
        "BOOTSTRAP_RESULT 15 passed; 0 failed; 0 ignored (real DO schema/enrollment fixtures; no normal HTTP routing/admission, copy apply or mailbox delivery claim)",
        "runtime-only",
        2,
    ),
    "access-carryover": WorkerdScenario(
        "session_scope_workerd",
        "ACCESS_CARRYOVER_RESULT 4 passed; 0 failed; 0 ignored (shared core HTTP verifier, real WebCrypto and fixture JWKS; zero age, key profile, verifier control and same-token expiry; no revocation claim)",
        "mode-last",
    ),
    "access-sessions": WorkerdScenario(
        "session_scope_workerd",
        "ACCESS_SESSION_RESULT 1 passed; 0 failed; 0 ignored (normal shared edge, signed HTTP and sockets, real JWKS, exact binding refusal and live revocation)",
        "mode-last",
    ),
    "session-carryover": WorkerdScenario(
        "session_scope_workerd",
        "SESSION_CARRYOVER_RESULT 1 passed; 0 failed; 0 ignored (normal authenticated upgrade, exact Welcome, same-session connection replacement and physical close; no copy-path claim)",
        "mode-last",
    ),
    "session-capabilities": WorkerdScenario(
        "session_scope_workerd",
        "SESSION_CAPABILITY_RESULT 2 passed; 0 failed; 0 ignored (normal enrollment and session, real workers-rs attachment roundtrip and original origin, actual same-Connection-object alarm; old SqlTx pointer identity retired)",
        "mode-last",
    ),
    "session-failures": WorkerdScenario(
        "session_scope_workerd",
        "SESSION_FAILURE_RESULT 5 passed; 0 failed; 0 ignored (normal enrolled sessions, targeted real postcommit publication failure, same-socket Register refusal, callback/SQL rollback and real retry, exact public journal/ordered socket barrier)",
        "mode-last",
    ),
    "holder-lifecycle": WorkerdScenario(
        "session_scope_workerd",
        "HOLDER_LIFECYCLE_RESULT 5 passed; 0 failed; 0 ignored (normal enrolled sessions and real Registry holder journal; readiness/expiry/replacement, exact frame/connection, close/alarm/late answer, held schema publication; fifth group combines existing hub primitive after normal admission with changed-SessionId wire close)",
        "mode-last",
    ),
    "raw-submit-refusal": WorkerdScenario(
        "session_scope_workerd",
        "RAW_SUBMIT_RESULT 1 passed; 0 failed; 0 ignored (normal admitted sockets, permanent raw Submit refusal, same-socket survival and empty recipient custody)",
        "mode-last",
    ),
    "sender-settlement": WorkerdScenario(
        "session_scope_workerd",
        "SENDER_SETTLEMENT_RESULT 1 passed; 0 failed; 0 ignored (normal Query and Settle through Registry to recipient Mailbox, known custody replay, unknown tombstone, late Submit refusal and custody readback)",
        "mode-last",
    ),
    "sender-retirement": WorkerdScenario(
        "session_scope_workerd",
        "SENDER_RETIREMENT_RESULT 1 passed; 0 failed; 0 ignored (verified sender fencing, actual recipient Mailbox, same-CallId receipt replay and one control effect)",
        "mode-last",
    ),
    "http-body-keepalive": WorkerdScenario(
        "session_scope_workerd",
        "HTTP_BODY_KEEPALIVE_RESULT 1 passed; 0 failed; 0 ignored (actual production enroll route, exactly2MiB unauthorized body with withheld suffix, two sends on one physical TCP connection, no unauthorized peer reservation)",
        "mode-last",
    ),
    "http-body-over-limit": WorkerdScenario(
        "session_scope_workerd",
        "HTTP_BODY_OVER_LIMIT_RESULT 1 passed; 0 failed; 0 ignored (eight actual HTTP route/auth/method variants,2MiB+1 body,413 and physical close for each; one grouped scenario)",
        "mode-last",
    ),
    "creation-gates": WorkerdScenario(
        "session_scope_workerd",
        "CREATION_GATES_RESULT 1 passed; 0 failed; 0 ignored (normal enrolled sessions and real Fleet/Registry/Copy; four creation gates, same-attempt retry/replay, first piece and held Claim refusal; no imported positive Claim or physical restart claim)",
        "mode-last",
    ),
    "delivery-config": WorkerdScenario(
        "session_scope_workerd",
        "DELIVERY_CONFIG_RESULT 1 passed; 0 failed; 0 ignored (explicit relay distinct from receiver, exact first/replay/update/readmission answers through real Registry and Mailbox; no Router delivery-call closure claim)",
        "mode-last",
    ),
    "fleet-notices": WorkerdScenario(
        "session_scope_workerd",
        "FLEET_NOTICES_RESULT 1 passed; 0 failed; 0 ignored (three normal admitted sessions; two reader installations with two exact numbered hints each, Fleet mutation and replay, own alarm, real Delivery and positioned notice read; no physical restart claim)",
        "mode-last",
    ),
    "copy-owner": WorkerdScenario(
        "session_scope_workerd",
        "COPY_OWNER_RESULT 1 passed; 0 failed; 0 ignored (owner HTTP Health before initialization, target bytes, empty Page, real Repair, machine refusal and minimal retired receipt; fixture Copy creation/retirement, no migration-fault, populated-page or full Registry admin claim)",
        "mode-last",
    ),
    "owner-registry": WorkerdScenario(
        "session_scope_workerd",
        "OWNER_REGISTRY_RESULT 3 passed; 0 failed; 0 ignored (normal Fleet creation, Page/Migrate, holder Release/Copy fence/target renewal, unchanged preview/tamper/takeover/replay and held-lease refusal; no successful retirement or physical restart claim)",
        "mode-last",
    ),
    "owner-retirement": WorkerdScenario(
        "session_scope_workerd",
        "OWNER_RETIREMENT_RESULT 1 passed; 0 failed; 0 ignored (conformance import/unheld/seed setup on actual composed stores, normal HTTP retirement, before-row/read-after-retired, exact Fleet acknowledgement and stable receipt/replay/Migrate; D1b HTTP import, physical file row count and process restart unobserved)",
        "mode-last",
    ),
}


class WorkerdObservation(BaseModel):
    model_config: t.ClassVar[ConfigDict] = ConfigDict(frozen=True)
    mode: str
    example: str
    completion_marker: str
    completion_marker_occurrences: int
    reaped_processes: int


class OwnerRetirementReadback(BaseModel):
    model_config: t.ClassVar[ConfigDict] = ConfigDict(frozen=True)
    database: Path
    rows: t.Literal[0]
    sha256: str


def read_owner_retirement_state(persistence: Path) -> OwnerRetirementReadback:
    """Read the fixture's project table only after its owning workerd has settled."""
    matches: list[tuple[Path, int]] = []
    for database in sorted(persistence.rglob("*.sqlite")):
        try:
            with closing(
                sqlite3.connect(database.resolve().as_uri() + "?mode=ro", uri=True)
            ) as connection:
                present = t.cast(
                    tuple[int],
                    connection.execute(
                        "SELECT count(*) FROM sqlite_schema WHERE type='table' AND name='owner_retirement_rows'"
                    ).fetchone(),
                )[0]
                if present:
                    rows = t.cast(
                        tuple[int],
                        connection.execute(
                            "SELECT count(*) FROM owner_retirement_rows"
                        ).fetchone(),
                    )[0]
                    matches.append((database, rows))
        except sqlite3.Error as error:
            raise ValueError(f"cannot read settled Worker state: {database}") from error
    if len(matches) != 1:
        raise ValueError("retirement requires exactly one physical fixture table")
    database, rows = matches[0]
    if rows != 0:
        raise ValueError(f"retirement retained {rows} physical project rows")
    with database.open("rb") as contents:
        digest = hashlib.file_digest(contents, "sha256").hexdigest()
    return OwnerRetirementReadback(database=database.resolve(), rows=0, sha256=digest)


def retired_scenarios() -> dict[str, str]:
    return {
        **{f"launcher:{name}": reason for name, reason in RETIRED_LAUNCHERS.items()},
        **{f"native:{name}": reason for name, reason in RETIRED_NATIVE_TARGETS.items()},
        **{
            f"workerd:{name}": reason
            for name, reason in RETIRED_WORKERD_SCENARIOS.items()
        },
    }


def workerd_scenarios(modes: tuple[str, ...]) -> list[tuple[str, WorkerdScenario]]:
    if not modes or len(set(modes)) != len(modes):
        raise ValueError("select at least one Worker scenario, each only once")
    selected: list[tuple[str, WorkerdScenario]] = []
    for mode in modes:
        if mode in RETIRED_WORKERD_SCENARIOS:
            raise ValueError(f"{mode}: {RETIRED_WORKERD_SCENARIOS[mode]}")
        if mode not in WORKERD_SCENARIOS:
            raise ValueError(f"unknown Worker scenario: {mode}")
        selected.append((mode, WORKERD_SCENARIOS[mode]))
    return selected


def worker_build_command(worker_build: Path) -> list[str]:
    """Build only the dev conformance image, including the current fault controls."""
    return [
        str(worker_build),
        "crates/collaboration-central-cloudflare",
        "--dev",
        "--no-opt",
        "--",
        "--locked",
        "--features",
        "conformance-faults",
    ]


def workerd_command(
    mode: str,
    node: str,
    miniflare: Path,
    persistence: Path,
    port: int,
    *,
    delivery_relay_peer: str | None = None,
) -> list[str]:
    _, scenario = workerd_scenarios((mode,))[0]
    runtime_arguments = [node, str(miniflare), str(persistence), str(port)]
    match scenario.argument_order:
        case "mode-first":
            arguments = [mode, *runtime_arguments]
        case "mode-last":
            arguments = [*runtime_arguments, mode]
        case "runtime-only":
            arguments = runtime_arguments
    if mode == "delivery-config":
        try:
            relay = UUID(delivery_relay_peer) if delivery_relay_peer else None
        except ValueError as error:
            raise ValueError(
                "delivery-config requires an explicit UUIDv7 relay"
            ) from error
        if relay is None or relay.version != 7:
            raise ValueError("delivery-config requires an explicit UUIDv7 relay")
        arguments.append(str(relay))
    return [
        "cargo",
        "run",
        "--locked",
        "-p",
        "collaboration-central-tokio",
        "--features",
        "conformance",
        "--example",
        scenario.example,
        "--",
        *arguments,
    ]


def check_workerd_output(mode: str, output: str) -> WorkerdObservation:
    _, scenario = workerd_scenarios((mode,))[0]
    lines = output.splitlines()
    occurrences = lines.count(scenario.marker)
    if occurrences == 0:
        raise ValueError(f"{mode} lacks its scenario completion marker")
    reaped = lines.count("WORKERD owned process stopped and reaped")
    if reaped < scenario.minimum_reaped_processes:
        raise ValueError(f"{mode} lacks its owned process/restart settlement evidence")
    return WorkerdObservation(
        mode=mode,
        example=scenario.example,
        completion_marker=scenario.marker,
        completion_marker_occurrences=occurrences,
        reaped_processes=reaped,
    )
