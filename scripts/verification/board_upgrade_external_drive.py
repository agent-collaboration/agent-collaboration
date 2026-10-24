"""OB5 on a declared external central; no deploy, enrollment or server lifecycle.

Run with board_upgrade_journey --external-plan PLAN.json. The endpoint must be
explicitly commissioned. This driver never builds, deploys, enrolls, reads home
stores, stops central, or supplies model input. Synthetic legacy copies only.
"""

import contextlib
import json
import shutil
import sqlite3
import typing as t
from pathlib import Path

from .board_central_runtime import private_json
from .board_journey_runtime import BoardRuntime, JourneyReceipt
from .board_journey_target import JourneyTarget
from .board_journey_values import file_digest
from .board_upgrade_cli import fingerprint
from .board_upgrade_external_cases import (
    claim_and_write,
    fleet_records,
    native_reference,
    observe_snapshots,
    owner_state,
    published_owner_state,
    verify_owner,
)
from .board_upgrade_external_fixture import clone_fixtures
from .board_upgrade_external_plan import ExternalPlan
from .board_upgrade_external_target import (
    qualify_external_target,
    require_short_socket_path,
    verify_external_source,
)
from .board_upgrade_external_transport import MachineReadback, OwnerEndpoint, OwnerProxy
from .board_upgrade_native_oracle import Checks
from .board_upgrade_publication_cli import PublicationCli, PublicationEndpoint
from .board_upgrade_publication_runtime import CompleteGate
from .board_upgrade_publication_views import PublicationPlan, Uploaded
from .board_upgrade_views import (
    DECLARED_PROVENANCE,
    DryRun,
    Object,
    UpgradeReport,
    json_object,
    text_field,
)


@t.final
class ExternalDrive:
    def __init__(self, plan: ExternalPlan) -> None:
        self.plan = plan
        self.driver_files = sorted(
            (plan.repository / "scripts/verification").glob(
                "board_upgrade_external*.py"
            )
        ) + [
            plan.repository / "scripts/verification" / name
            for name in [
                "board_upgrade_publication_cli.py",
                "board_upgrade_publication_runtime.py",
                "board_journey_runtime.py",
                "board_central_runtime.py",
                "board_upgrade_native_oracle.py",
                "board_upgrade_source.py",
            ]
        ]
        self.driver_hashes = {
            str(path): file_digest(path) for path in self.driver_files
        }
        target_receipt = qualify_external_target(plan)
        self.receipt = JourneyReceipt(plan.output_directory)
        plan.output_directory.chmod(0o700)
        self.receipt.target = target_receipt
        self.owner = OwnerEndpoint.open(plan)
        self.machine = plan.machine()
        self.checks = Checks()
        self.originals: dict[str, Object] = {}
        self.proxy: OwnerProxy | None = None
        self.root = plan.output_directory
        self.private = plan.runtime_parent / (self.root.name + "-private")
        self.private.mkdir(mode=0o700)
        try:
            dummy = self.private / "transport-assertion"
            _ = dummy.write_text("local-driver-transport-only")
            dummy.chmod(0o600)
            self.endpoint_assertion = dummy
            target = JourneyTarget(
                plan.repository, plan.expected_head, None, plan.artifacts
            )
            target.launcher = plan.artifact("board-debug-runtime").path
            target.cli = plan.artifact("agent-collaboration").path
            target.digests = {
                "launcherSha256": plan.launcher_sha256,
                "cliSha256": plan.cli_sha256,
            }
            self.runtime = BoardRuntime(target, self.receipt, self.private)
            require_short_socket_path(self.runtime.directory)
            private_json(
                self.runtime.directory / "service-identity.json",
                {"version": 1, "serviceId": self.machine["installation"]},
            )
            private_json(
                self.runtime.directory / "installation-peer.json",
                {"version": 1, "peerId": self.machine["peer"]},
            )
            session = self.runtime.directory / "fixture-session.json"
            private_json(
                session,
                {
                    "endpoint": plan.machine_endpoint or plan.endpoint + "/v1/session",
                    "access": {
                        "clientId": self.machine["access_client_id"],
                        "clientSecret": self.machine["access_client_secret"],
                    },
                    "credential": "Bearer " + self.machine["credential"],
                    "peer": self.machine["peer"],
                    "receiver": self.machine["receiver"],
                    "relay": self.machine["relay"],
                },
            )
            self.runtime.fixture_session = session
            cli_plan = PublicationPlan(
                repository=plan.repository,
                expected_head=plan.expected_head,
                binary=plan.binary,
                binary_sha256=plan.binary_sha256,
                central_binary=plan.binary,
                central_sha256=plan.binary_sha256,
                copy_root=self.root,
                sources=[self.root],
                source_evidence="syntheticFixture",
                output_directory=self.root,
                timeout_seconds=plan.timeout_seconds,
            )
            self.cli = PublicationCli(
                cli_plan,
                self.checks,
                self.originals,
                check_source=False,
                binary_directory=plan.artifacts,
                pre_signed=True,
            )
        except BaseException:
            # Construction starts no process; remove only our newly created credential root.
            shutil.rmtree(self.private)
            self.receipt.close()
            raise
        self.case_results: list[Object] = []
        self.claim_input: tuple[Path, Path, UpgradeReport] | None = None
        self.active_case: str | None = None

    def wire(self) -> MachineReadback:
        return MachineReadback(
            self.plan.machine_endpoint or self.plan.endpoint,
            {
                "Authorization": "Bearer " + self.machine["credential"],
                "CF-Access-Client-Id": self.machine["access_client_id"],
                "CF-Access-Client-Secret": self.machine["access_client_secret"],
            },
        )

    def case(self, label: str, name: str) -> tuple[Path, Path]:
        case = self.root / label
        case.mkdir(mode=0o700)
        source = clone_fixtures(self.plan.repository, case / "sources", [name])[0]
        self.originals[str(source)] = fingerprint(source)
        return case, source

    def cli_run(
        self,
        case: Path,
        label: str,
        source: Path,
        *,
        dry: bool = False,
        expected_exit: int = 0,
        relabel: bool = False,
        gate: CompleteGate | None = None,
        additional_sources: tuple[Path, ...] = (),
    ) -> DryRun | Uploaded | None:
        verify_external_source(self.plan)
        if self.proxy is None:
            raise ValueError("owner transport missing")
        return self.cli.run(
            case,
            label,
            source,
            None
            if dry
            else PublicationEndpoint(self.proxy.url, self.endpoint_assertion),
            machine_label=str(DECLARED_PROVENANCE["machine"]),
            expected_exit=expected_exit,
            relabel=relabel,
            gate=gate,
            additional_sources=additional_sources,
        )

    def publication(self, name: str, label: str, phase: str | None = None) -> None:
        self.active_case = label
        start = len(self.checks.records)
        case, source = self.case(label, name)
        self.proxy = OwnerProxy(self.owner, after_publication_fault=phase == "after")
        interrupted_record: Object | None = None
        try:
            dry = self.cli_run(case, "dry-run", source, dry=True)
            if not isinstance(dry, DryRun):
                raise TypeError("synthetic dry run unavailable")
            first_project = dry.report.projects[0]
            readiness = self.owner.call(
                f"/admin/v1/import/projects/{first_project.project}",
                {"kind": "status", "value": {"digest": first_project.digest}},
            )
            self.checks.equal(
                "authenticated owner route proves fresh target readiness",
                {"kind": "refused", "value": {"kind": "notFound"}},
                readiness,
            )
            _ = (case / "target-readiness.json").write_text(
                json.dumps(readiness, indent=2)
            )
            with contextlib.closing(self.wire()) as machine:
                _ = machine.fleet(True)
                baseline = machine.snapshot_cut
            if baseline is None:
                raise ValueError("machine snapshot did not establish a pre-upload cut")
            expected = native_reference(
                self.plan.repository,
                self.plan.native_binary,
                source,
                case,
                digest=self.plan.native_sha256,
            )
            self.checks.equal(
                "dry-run logical project digests match native preparation",
                [p.digest for p in expected.report.projects],
                [p.digest for p in dry.report.projects],
            )
            if phase == "before":
                gate = CompleteGate(self.proxy.url, "before")
                try:
                    _ = self.cli_run(case, "interrupted", source, gate=gate)
                finally:
                    gate.close()
                pending = owner_state(self.owner, dry.report)
                first = self.owner.call(
                    f"/admin/v1/import/projects/{dry.report.projects[0].project}",
                    {
                        "kind": "status",
                        "value": {"digest": dry.report.projects[0].digest},
                    },
                )
                self.checks.equal(
                    "before-publication workflow reserved",
                    {"kind": "reserved"},
                    json_object.validate_python(first["value"])["phase"],
                )
                interrupted_record = json_object.validate_python(first["value"])
                first_health = json_object.validate_python(
                    json_object.validate_python(
                        pending[dry.report.projects[0].project]
                    )["health"]
                )
                self.checks.equal(
                    "before-publication Copy is not Following",
                    False,
                    json_object.validate_python(
                        json_object.validate_python(first_health["value"])["stage"]
                    ).get("value")
                    == "following",
                )
                self.checks.equal(
                    "every declared page staged before interruption",
                    json_object.validate_python(interrupted_record["header"])["pages"],
                    sum(row["requestKind"] == "page" for row in self.proxy.responses),
                )
                _ = (case / "before-publication.json").write_text(
                    json.dumps(pending, indent=2)
                )
            elif phase == "after":
                _ = self.cli_run(case, "interrupted", source, expected_exit=2)
                first = self.owner.call(
                    f"/admin/v1/import/projects/{dry.report.projects[0].project}",
                    {
                        "kind": "status",
                        "value": {"digest": dry.report.projects[0].digest},
                    },
                )
                self.checks.equal(
                    "interior fault is after publication before completion",
                    "published",
                    json_object.validate_python(
                        json_object.validate_python(first["value"])["phase"]
                    )["kind"],
                )
                interrupted_record = json_object.validate_python(first["value"])
                self.checks.equal(
                    "Copy publication reply actually dropped",
                    "dropped",
                    json_object.validate_python(
                        json_object.validate_python(self.proxy.completed[0]["fault"])[
                            "objectReply"
                        ]
                    )["kind"],
                )
                _ = (case / "after-publication.json").write_text(
                    json.dumps(
                        {"status": first, "wire": self.proxy.completed[0]}, indent=2
                    )
                )
            resumed = self.cli_run(case, "upload", source)
            if not isinstance(resumed, Uploaded):
                raise TypeError("actual admin upload unavailable")
            self.checks.equal(
                "every project publication answered",
                len(dry.report.projects),
                len(resumed.publication.projects),
            )
            state = published_owner_state(self.owner, dry.report)
            verify_owner(self.checks, dry.report, state)
            if interrupted_record is not None:
                resumed_state = json_object.validate_python(
                    json_object.validate_python(state[dry.report.projects[0].project])[
                        "import"
                    ]
                )
                self.checks.equal(
                    "interrupted attempt retains identity and first physical header",
                    [interrupted_record["importId"], interrupted_record["header"]],
                    [
                        json_object.validate_python(resumed_state["value"])["importId"],
                        json_object.validate_python(resumed_state["value"])["header"],
                    ],
                )
            _ = (case / "published-owner-state.json").write_text(
                json.dumps(state, indent=2)
            )
            with contextlib.closing(self.wire()) as machine:
                observe_snapshots(machine, source, expected, self.checks, case)
                first_fleet = fleet_records(
                    machine, dry.report, self.checks, case, after=baseline
                )
            replay = self.cli_run(case, "replay", source, relabel=True)
            if not isinstance(replay, Uploaded):
                raise TypeError("same-input replay unavailable")
            self.checks.equal(
                "same input never publishes twice",
                ["alreadyImported"] * len(dry.report.projects),
                [p.answer.kind for p in replay.publication.projects],
            )
            self.checks.equal(
                "later wall-clock keeps fleet logical digest",
                dry.report.fleet_digest,
                replay.report.fleet_digest,
            )
            after = owner_state(self.owner, dry.report)
            self.checks.equal(
                "replay retains every import identity/header/phase",
                {k: json_object.validate_python(v)["import"] for k, v in state.items()},
                {k: json_object.validate_python(v)["import"] for k, v in after.items()},
            )
            with contextlib.closing(self.wire()) as machine:
                second_fleet = fleet_records(
                    machine, dry.report, self.checks, case, after=baseline
                )
            self.checks.equal(
                "later fleet feed/snapshot bytes unchanged", first_fleet, second_fleet
            )
            changed = case / "changed.sqlite"
            _ = shutil.copyfile(source, changed)
            with contextlib.closing(sqlite3.connect(changed)) as connection:
                _ = connection.execute(
                    "UPDATE board_topics SET description=description || ' changed-content'"
                )
                connection.commit()
            self.originals[str(changed)] = fingerprint(changed)
            _ = self.cli_run(case, "changed-content", changed, expected_exit=2)
            self.checks.equal(
                "changed content is importConflict",
                True,
                any(
                    json_object.validate_python(row["reply"]).get("kind")
                    == "importConflict"
                    for row in self.proxy.responses
                ),
            )
            conflicts = [
                json_object.validate_python(row["reply"])
                for row in self.proxy.responses
                if json_object.validate_python(row["reply"]).get("kind")
                == "importConflict"
            ]
            for conflict in conflicts:
                content = json_object.validate_python(conflict["value"])
                self.checks.equal(
                    "content conflict names different existing and offered digests",
                    True,
                    content["existing"] is not None
                    and content["existing"] != content["offered"],
                )
            refused_state = owner_state(self.owner, dry.report)
            self.checks.equal(
                "changed content preserves original import workflow",
                {k: json_object.validate_python(v)["import"] for k, v in state.items()},
                {
                    k: json_object.validate_python(v)["import"]
                    for k, v in refused_state.items()
                },
            )
            if phase is None and self.claim_input is None:
                self.claim_input = (case, source, dry.report)
            self.checks.equal(
                "owner proxy no transport failure", None, self.proxy.failure
            )
            self.case_results.append(
                {
                    "case": label,
                    "observed": True,
                    "passed": sum(c.passed for c in self.checks.records[start:]),
                    "failed": sum(not c.passed for c in self.checks.records[start:]),
                }
            )
        finally:
            self.proxy.close()
            self.proxy = None
            self.save("running")

    def collision(self) -> None:
        self.active_case = "collision"
        case = self.root / "collision"
        case.mkdir(mode=0o700)
        sources = clone_fixtures(
            self.plan.repository,
            case / "sources",
            ["legacy-complete.sqlite", "collision.sqlite"],
        )
        self.originals.update({str(p): fingerprint(p) for p in sources})
        self.proxy = OwnerProxy(self.owner)
        try:
            _ = self.cli_run(
                case,
                "collision",
                sources[0],
                expected_exit=2,
                additional_sources=tuple(sources[1:]),
            )
            report = json_object.validate_json(
                (case / "collision-report.json").read_bytes()
            )
            self.checks.equal(
                "collision refuses before contacting central", [], self.proxy.responses
            )
            self.checks.equal(
                "collision refusal is explicit",
                "name collisions refuse publication",
                report["reason"],
            )
            collisions = json_object.validate_python(report["report"])["collisions"]
            if not isinstance(collisions, list):
                raise TypeError("collision report not an array")
            self.checks.equal(
                "both collision kinds retain conflicting IDs",
                ["board", "workspace"],
                sorted(
                    text_field(json_object.validate_python(c), "kind")
                    for c in collisions
                ),
            )
            self.checks.equal(
                "each collision retains both IDs without renaming",
                [2, 2],
                sorted(
                    len(ids) if isinstance(ids, list) else -1
                    for c in collisions
                    for ids in [json_object.validate_python(c)["ids"]]
                ),
            )
            self.case_results.append({"case": "collision", "observed": True})
        finally:
            self.proxy.close()
            self.proxy = None

    def claim(self) -> None:
        self.active_case = "real-claim-first-write"
        if self.claim_input is None:
            raise ValueError("positive publication must precede Claim")
        case, source, report = self.claim_input
        self.proxy = OwnerProxy(self.owner)
        try:
            project = claim_and_write(
                self.runtime, report, self.checks, case, owner=self.owner
            )
            held = owner_state(self.owner, report)
            _ = self.cli_run(case, "post-claim-replay", source, relabel=True)
            held_after = owner_state(self.owner, report)

            def authority(state: Object) -> Object:
                health = json_object.validate_python(
                    json_object.validate_python(state[project])["health"]
                )
                return json_object.validate_python(
                    json_object.validate_python(health["value"])["copy"]
                )

            self.checks.equal(
                "import replay never changes claimed Copy authority",
                authority(held),
                authority(held_after),
            )
            self.case_results.append(
                {"case": "real-claim-first-write", "observed": True}
            )
        finally:
            self.proxy.close()
            self.proxy = None

    def save(self, outcome: str) -> None:
        _ = (self.root / "external-journey.json").write_text(
            json.dumps(
                {
                    "outcome": outcome,
                    "plan": json_object.validate_json(self.plan.model_dump_json()),
                    "driverHashesAtEntry": self.driver_hashes,
                    "driverHashesAtSave": {
                        str(path): file_digest(path) for path in self.driver_files
                    },
                    "cases": self.case_results,
                    "activeCase": self.active_case,
                    "unobservedCases": sorted(
                        {
                            "legacy-complete",
                            "role-history",
                            "mixed-watch-scope",
                            "before-publication",
                            "after-publication",
                            "collision",
                            "real-claim-first-write",
                        }
                        - {text_field(row, "case") for row in self.case_results}
                    ),
                    "passed": sum(c.passed for c in self.checks.records),
                    "failed": sum(not c.passed for c in self.checks.records),
                    "assertions": [
                        c.model_dump(mode="json") for c in self.checks.records
                    ],
                    "commands": self.cli.commands,
                    "limits": [
                        "real Access sign-in/TLS/edge headers/deployed Worker durability only when explicitly run on staging",
                        "local prebuilt qualification is not current-source acceptance",
                    ],
                },
                indent=2,
            )
        )

    def close(self) -> None:
        failures: list[str] = []
        jobs: list[tuple[str, t.Callable[[], None]]] = [
            ("host", self.runtime.stop),
            ("cli", self.cli.close),
        ]
        if self.proxy is not None:
            jobs.append(("ownerProxy", self.proxy.close))
        for label, settle in jobs:
            try:
                settle()
            except Exception as error:  # noqa: BLE001 -- settle other owners and retain redacted cleanup evidence
                failures.append(label + ":" + type(error).__name__)
        try:
            self.checks.equal(
                "all synthetic source bytes held",
                self.originals,
                {p: fingerprint(Path(p)) for p in self.originals},
            )
        except (AssertionError, OSError):
            failures.append("sourceFingerprint")
        # Machine credential files and the transport sentinel are owned scratch;
        # delete them only once every exact consumer has settled.
        settled = (
            self.runtime.child is None
            and self.cli.active is None
            and (self.proxy is None or not self.proxy.thread.is_alive())
        )
        if settled:
            shutil.rmtree(self.private)
        self.receipt.record(
            {
                "kind": "cleanup",
                "privateRuntimeDeleted": not self.private.exists(),
                "externalCentralStopped": False,
                "enrollmentPerformed": False,
                "failures": list(failures),
            }
        )
        self.receipt.close()
        if failures or not settled:
            raise RuntimeError(
                "external OB5 cleanup incomplete; inspect retained cleanup receipt"
            )
