"""The run context every native-capabilities journey shares.

A Journey qualifies the declared candidate once, owns one fresh private evidence
directory, and opens each installed-Codex drive with the D1 reference launch and the
client's own per-invocation session recorder.
"""

import os
from pathlib import Path

from pydantic import BaseModel, JsonValue

from scripts.verification.acp_metadata_observation import ProviderObservation, save_raw
from scripts.verification.native_capabilities_models import (
    NativeCapabilitiesPlan,
    OperationRow,
    RpcReply,
)
from scripts.verification.native_capabilities_target import (
    ObservationCursor,
    QualifiedTarget,
    consumer_requests,
    endpoint_socket,
    observe_session,
    operation_rows,
    qualify_target,
)
from scripts.verification.native_capabilities_terminal import (
    TAB,
    InstalledCodexTerminal,
)


class JourneyResult(BaseModel):
    journey: str
    checks: dict[str, JsonValue]
    limits: list[str]


class Journey:
    """One journey run: the qualified candidate, its fixture and a fresh evidence directory."""

    def __init__(self, plan: NativeCapabilitiesPlan, output: Path) -> None:
        output.mkdir(mode=0o700)
        self.output: Path = output
        save_raw(output / "binding.json", plan.model_dump_json(indent=2))
        self.target: QualifiedTarget = qualify_target(plan, output)
        save_raw(output / "fixture.json", self.target.fixture.model_dump_json(indent=2))

    def terminal(self, label: str) -> InstalledCodexTerminal:
        # D1 reference launch: per-invocation mentions_v2=false puts skills and plugins
        # under `$`. The client's own recorder logs its outbound turn items to a fresh
        # file here; both settings are per-invocation, global configuration is untouched.
        environment = dict(os.environ)
        environment["CODEX_TUI_RECORD_SESSION"] = "1"
        environment["CODEX_TUI_SESSION_LOG_PATH"] = str(self.recorder(label))
        return InstalledCodexTerminal(
            [
                str(self.target.plan.codex_cli),
                "--no-alt-screen",
                "--remote",
                f"unix://{endpoint_socket(self.target)}",
                "--cd",
                str(self.target.fixture.workspace),
                "-c",
                "features.mentions_v2=false",
                "resume",
                self.target.plan.session_id,
            ],
            self.output / f"{label}.pty",
            environment=environment,
        )

    def recorder(self, label: str) -> Path:
        return self.output / f"{label}.session.jsonl"

    def operations(self, label: str) -> list[OperationRow]:
        return operation_rows(self.target, self.output, label)

    def observe(
        self, label: str, cursor: ObservationCursor | None
    ) -> ProviderObservation:
        return observe_session(self.target, self.output, label, cursor)

    def requests(
        self, label: str, requests: list[tuple[str, dict[str, JsonValue]]]
    ) -> list[RpcReply]:
        return consumer_requests(self.target, requests, self.output, label)

    def select_skill(
        self, codex: InstalledCodexTerminal, prefix: str, name: str, row: str
    ) -> None:
        """Type `$` and part of the name, wait for the picker row, select it with Tab."""
        mark = codex.mark()
        codex.type_text(prefix + "$" + name[:-4])
        save_raw(self.output / f"picker-{name}.txt", codex.wait_for(row, mark, 30))
        codex.send(TAB)
