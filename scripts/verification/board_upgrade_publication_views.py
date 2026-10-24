"""Parse the CLI/wire/owning-store evidence and check adopted OB5 publication rules."""

import typing as t
from pathlib import Path

from pydantic import BaseModel, ConfigDict, Field, TypeAdapter

from .board_upgrade_native_oracle import Checks
from .board_upgrade_views import (
    DryRun,
    Object,
    Refused,
    UpgradeReport,
    View,
    json_object,
)

type Scenario = t.Literal[
    "all", "dry-run", "publication", "before-publication", "after-publication"
]


class PublicationPlan(BaseModel):
    model_config: t.ClassVar[ConfigDict] = ConfigDict(extra="forbid", frozen=True)
    repository: Path
    expected_head: str = Field(pattern=r"^[0-9a-f]{40}$")
    binary: Path
    binary_sha256: str = Field(pattern=r"^[0-9a-f]{64}$")
    central_binary: Path
    central_sha256: str = Field(pattern=r"^[0-9a-f]{64}$")
    copy_root: Path
    sources: list[Path] = Field(min_length=1)
    source_evidence: t.Literal["syntheticFixture", "ownerSuppliedCopy"]
    output_directory: Path
    timeout_seconds: int = Field(default=180, ge=30, le=3600)


class Position(View):
    epoch: int
    sequence: int


class ImportedPayload(View):
    head: Position


class Imported(View):
    kind: t.Literal["imported"]
    value: ImportedPayload


class AlreadyImported(View):
    kind: t.Literal["alreadyImported"]


class ProjectPublication(View):
    project: str
    answer: t.Annotated[Imported | AlreadyImported, Field(discriminator="kind")]


class UploadResults(View):
    projects: list[ProjectPublication]


class Uploaded(View):
    kind: t.Literal["uploaded"]
    report: UpgradeReport
    publication: UploadResults


publication_report = TypeAdapter(
    t.Annotated[DryRun | Uploaded | Refused, Field(discriminator="kind")]
)


class Ready(View):
    kind: t.Literal["ready"]
    url: str
    pid: int


class Inspected(View):
    kind: t.Literal["inspected"]
    output: Path


class Stopped(View):
    kind: t.Literal["stopped"]
    output: Path


class CentralSettlement(View):
    arguments: list[str]
    pid: int
    exit: int
    stop_receipt_observed: bool


central_reply = TypeAdapter(
    t.Annotated[Ready | Inspected | Stopped, Field(discriminator="kind")]
)


class CopyReadback(View):
    database: str
    record: list[str]
    seed: list[str]
    summary: list[str]
    posts: list[str]
    staged: list[str]


class CentralReadback(View):
    copies: list[CopyReadback]
    workflows: list[str]
    directory: list[str]


class Manifest(View):
    database: str
    position: Position
    schema_: Object = Field(alias="schema")


class Header(View):
    digest: str
    declared_owner: str = Field(alias="declaredOwner")
    provenance: Object
    manifest: Manifest
    summary: Object


class Workflow(View):
    import_id: str = Field(alias="importId")
    project: str
    digest: str
    header: str
    phase: str


class CopyAuthority(View):
    holder: str | None
    fence: int
    acked: Position
    schema_: Object = Field(alias="schema")


class FollowingCopy(View):
    state: t.Literal["following"]
    authority: CopyAuthority = Field(alias="copy")


class DirectorySummary(View):
    database: str
    position: Position
    summary: Object


UNKNOWN_SUMMARY: t.Final[Object] = {
    "kind": "projectSummary",
    "value": {"status": None, "latestRecordAt": {"kind": "legacyUnknown"}},
}


def workflows(snapshot: CentralReadback) -> dict[str, Workflow]:
    rows = [Workflow.model_validate_json(row) for row in snapshot.workflows]
    if len({row.project for row in rows}) != len(rows):
        raise AssertionError("duplicate project workflows")
    return {row.project: row for row in rows}


def one(checks: Checks, label: str, rows: list[str]) -> str:
    checks.equal(label + " singleton", 1, len(rows))
    return rows[0]


def verify_published(
    checks: Checks,
    report: UpgradeReport,
    snapshot: CentralReadback,
    retained_headers: dict[str, Object],
) -> None:
    """Expected identities/counts are from legacy input; heads bind to the CLI/wire."""
    expected = {project.project: project for project in report.projects}
    checks.equal("nonempty project observation", True, bool(expected))
    checks.equal(
        "published copy identities",
        sorted(expected),
        sorted(copy.database for copy in snapshot.copies),
    )
    saved = workflows(snapshot)
    checks.equal("complete workflow identities", sorted(expected), sorted(saved))
    projected = {
        DirectorySummary.model_validate_json(row).database: row
        for row in snapshot.directory
    }
    checks.equal("directory projected every copy", sorted(expected), sorted(projected))
    for raw in snapshot.copies:
        project = expected[raw.database]
        workflow = saved[raw.database]
        header_object = json_object.validate_json(workflow.header)
        header = Header.model_validate(header_object)
        manifest = Manifest.model_validate(project.manifest)
        if raw.database in retained_headers:
            checks.equal(
                "first physical header retained",
                retained_headers[raw.database],
                header_object,
            )
            manifest = Header.model_validate(retained_headers[raw.database]).manifest
        else:
            checks.equal(
                "CLI manifest published", project.manifest, header_object["manifest"]
            )
            checks.equal(
                "declared owner persisted", report.owner, header.declared_owner
            )
            checks.equal(
                "declared provenance persisted",
                report.fleet.provenance,
                header.provenance,
            )
        checks.equal("semantic digest persisted", project.digest, workflow.digest)
        state = one(checks, "copy record", raw.record)
        checks.equal(
            "copy is actually Following",
            "following",
            json_object.validate_json(state)["state"],
        )
        copy = FollowingCopy.model_validate_json(state).authority
        checks.equal("import remains Unheld", None, copy.holder)
        checks.equal("initial fence is epoch one", 1, copy.fence)
        checks.equal(
            "published position matches physical header",
            manifest.position.model_dump(),
            copy.acked.model_dump(),
        )
        checks.equal(
            "published schema matches physical header", manifest.schema_, copy.schema_
        )
        checks.equal(
            "native post count matches legacy", [str(project.posts)], raw.posts
        )
        checks.equal("published pages consumed", ["0"], raw.staged)
        seed = json_object.validate_json(one(checks, "published seed", raw.seed))
        published_seed = json_object.validate_python(seed["published"])
        checks.equal(
            "seed receipt names the published position",
            manifest.position.model_dump(),
            published_seed["position"],
        )
        summary_json = one(checks, "atomic summary", raw.summary)
        summary = DirectorySummary.model_validate_json(summary_json)
        checks.equal("summary database", raw.database, summary.database)
        checks.equal(
            "summary position",
            manifest.position.model_dump(),
            summary.position.model_dump(),
        )
        checks.equal(
            "no invented imported time or status", UNKNOWN_SUMMARY, summary.summary
        )
        checks.equal(
            "header carries adopted imported summary", UNKNOWN_SUMMARY, header.summary
        )
        checks.equal(
            "Fleet projection equals atomic summary",
            summary_json,
            projected[raw.database],
        )
        checks.equal(
            "workflow complete after publication",
            {"kind": "complete", "position": manifest.position.model_dump()},
            json_object.validate_json(workflow.phase),
        )


def verify_unpublished(
    checks: Checks, snapshot: CentralReadback, header: Object
) -> str:
    expected = Header.model_validate(header)
    saved = workflows(snapshot)
    workflow = saved[expected.manifest.database]
    checks.equal(
        "unpublished header retained",
        header,
        json_object.validate_json(workflow.header),
    )
    checks.equal(
        "interrupted workflow remains Reserved",
        {"kind": "reserved"},
        json_object.validate_json(workflow.phase),
    )
    first = next(
        copy for copy in snapshot.copies if copy.database == expected.manifest.database
    )
    checks.equal("pre-publication has no live posts", [], first.posts)
    checks.equal("pre-publication has no atomic summary", [], first.summary)
    checks.equal(
        "pages staged before interruption",
        True,
        len(first.staged) == 1 and int(first.staged[0]) > 0,
    )
    return workflow.import_id
