"""Validated inputs and partial raw-result views for the D1a observer."""

import typing as t
from pathlib import Path

from pydantic import BaseModel, ConfigDict, Field, JsonValue, TypeAdapter

type Object = dict[str, JsonValue]
DECLARED_OWNER: t.Final[str] = "verification-owner"
DECLARED_PROVENANCE: t.Final[Object] = {
    "installation": "550e8400-e29b-41d4-a716-446655440000",
    "machine": "Verifier fixture machine",
}


class View(BaseModel):
    model_config: t.ClassVar[ConfigDict] = ConfigDict(
        extra="ignore", frozen=True, strict=True
    )


class ObservationPlan(View):
    repository: Path
    reference_repository: Path
    expected_head: str
    binary: Path
    binary_sha256: str
    native_binary: Path
    native_sha256: str
    output_directory: Path


class UpgradeJourneyArguments(View):
    expected_head: str | None
    output_directory: Path | None
    list_cases: bool
    observe_plan: Path | None
    publication_plan: Path | None
    external_plan: Path | None = None
    scenario: t.Literal[
        "all", "dry-run", "publication", "before-publication", "after-publication"
    ]


class Cursor(View):
    epoch: int
    position: int


class ProjectReport(View):
    project: str
    workspace: str
    board: str
    title: str
    description: str
    posts: int
    feed_rows: int = Field(alias="feedRows")
    old_to_new: dict[str, Cursor] = Field(alias="oldToNew")
    digest: str
    manifest: Object


class FleetBundle(View):
    provenance: Object
    records: list[Object]


class UpgradeReport(View):
    owner: str
    fleet: FleetBundle
    fleet_digest: str = Field(alias="fleetDigest")
    collisions: list[Object]
    projects: list[ProjectReport]
    workspace_counts: list[Object] = Field(alias="workspaceCounts")
    board_counts: list[Object] = Field(alias="boardCounts")
    skipped: list[str]
    refused: list[str]


class DryRun(View):
    kind: t.Literal["dryRun"]
    report: UpgradeReport


class Refused(View):
    kind: t.Literal["refused"]
    reason: str
    report: UpgradeReport | None


run_report = TypeAdapter(t.Annotated[DryRun | Refused, Field(discriminator="kind")])


class NativeInbox(View):
    reader: Object
    mode: t.Literal["unread", "latest"]
    post_ids: list[str] = Field(alias="postIds")


class NativePage(View):
    schema_steps: list[str] = Field(alias="schemaSteps")
    table: str
    rows: list[list[Object]]


class NativeProject(View):
    project: str
    pages: list[NativePage]
    inboxes: list[NativeInbox]


class NativeObservation(View):
    report: UpgradeReport
    projects: list[NativeProject]


class SourceRows(View):
    tables: dict[str, list[Object]]


class ColumnName(View):
    name: str


json_object = TypeAdapter(Object)
json_value = TypeAdapter(JsonValue)
string_list = TypeAdapter(list[str])


def text_field(row: Object, name: str) -> str:
    value = row[name]
    if not isinstance(value, str):
        raise TypeError(f"{name} is not text")
    return value


def int_field(row: Object, name: str) -> int:
    value = row[name]
    if not isinstance(value, int) or isinstance(value, bool):
        raise TypeError(f"{name} is not an integer")
    return value
