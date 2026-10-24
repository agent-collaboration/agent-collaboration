"""Observe M2b role authority, cardinality and exact CLI/MCP replay parity."""

import json
import typing as t

from .board_journey_fixture import BoardFixture, accepted_decision, human_actor
from .board_journey_runtime import decision_values, project_request
from .board_journey_values import (
    JourneyFailure,
    JsonObject,
    JsonValue,
    array_value,
    field,
    fixture_identity,
    object_value,
)


def paired_file_write(
    fixture: BoardFixture, command: list[str], tool: str, request: JsonObject
) -> JsonObject:
    """Execute a real CLI write, then replay the identical request over TCP MCP."""
    path = fixture.runtime.root / f"request-{request['requestId']}.json"
    _ = path.write_text(json.dumps(request), encoding="utf-8")
    cli = fixture.client.cli(
        [*command, "--request-file", str(path)], f"M2b CLI {' '.join(command)}"
    )
    mcp = fixture.client.read(tool, request)
    fixture.receipt.equal(
        "CLI and TCP MCP return the identical structured keyed decision",
        "M2b/V8/API-1/API-8/RC-4",
        (field(cli, "record", "result"), mcp.get("result")),
    )
    return mcp


def expect_reasons(
    fixture: BoardFixture, reply: JsonObject, reasons: list[JsonValue]
) -> None:
    decisions = decision_values(reply)
    fixture.receipt.equal(
        "refusal names the exact typed reason and requirement or holder",
        "M2b/V1/RL-5/RL-6/RL-7/TH-2",
        (
            [{"kind": "rejected", "reasons": reasons}],
            list[JsonValue](decisions),
        ),
    )


def role_required(requirement: JsonObject) -> JsonObject:
    return {
        "kind": "role",
        "reason": {"kind": "roleRequired", "requirement": requirement},
    }


def record_payloads(fixture: BoardFixture, tool: str) -> list[JsonObject]:
    result = fixture.client.read(
        tool, {"projectId": fixture.project, "page": {"limit": 100}}
    )
    fixture.receipt.equal(
        "M2b record page is complete at its explicit bound",
        "RC-5/M2b fixture bound",
        (None, result.get("nextCursor")),
    )
    return [
        object_value(field(record, "record"))
        for record in array_value(result["records"])
    ]


@t.final
class RoleScenario:
    """Own the fixture's expected assignment revisions, never infer them from output."""

    def __init__(self, fixture: BoardFixture) -> None:
        self.fixture = fixture
        self.revisions: dict[str, int] = {"project": 1, "workstream": 0}

    def change(
        self,
        operation: JsonObject,
        *,
        actor: JsonObject | None = None,
        scope: str = "workstream",
        changed: bool = True,
        reasons: list[JsonValue] | None = None,
    ) -> JsonObject:
        fixture = self.fixture
        revision = self.revisions[scope]
        operation = {
            "subject": {
                "kind": scope,
                "subject": fixture.project
                if scope == "project"
                else fixture.workstream,
            },
            "expectedRevision": revision,
            **operation,
        }
        kind = operation["kind"]
        verb = "grant" if kind == "roleGrant" else "release"
        before = record_payloads(fixture, "role_list")
        reply = paired_file_write(
            fixture,
            ["role", verb],
            f"role_{verb}",
            project_request(actor or fixture.owner, fixture.project, operation),
        )
        if reasons is not None:
            expect_reasons(fixture, reply, reasons)
        else:
            fixture.receipt.equal(
                "grant or release reports the intended change outcome",
                "RL-1/RL-2/M2b cardinality",
                (
                    "changed" if changed else "alreadySo",
                    accepted_decision(reply)["change"],
                ),
            )
        after = record_payloads(fixture, "role_list")
        if reasons is not None or not changed:
            fixture.receipt.equal(
                "refusal or AlreadySo changes no assignment or history",
                "RL-2/RC-4",
                (list[JsonValue](before), list[JsonValue](after)),
            )
        else:
            self.revisions[scope] = revision + 1
            for entry in array_value(
                field(accepted_decision(reply), "effects", "entries")
            ):
                reference = array_value(entry)[0]
                if field(reference, "kind") != "roleAssignment":
                    continue
                identity = str(field(reference, "id"))
                cli_record = fixture.client.cli(
                    ["role", "show", "--project-id", fixture.project, "--id", identity],
                    "M2b exact role record through Unix CLI",
                )
                mcp_record = fixture.client.read(
                    "role_show", {"projectId": fixture.project, "record": reference}
                )
                fixture.receipt.equal(
                    "both listeners return the complete identical stored assignment",
                    "V8/RL-2",
                    (object_value(cli_record["record"]), mcp_record),
                )
        return reply

    def grant(
        self,
        role: str,
        holder: JsonObject,
        *,
        actor: JsonObject | None = None,
        scope: str = "workstream",
        changed: bool = True,
        reasons: list[JsonValue] | None = None,
    ) -> JsonObject:
        return self.change(
            {
                "kind": "roleGrant",
                "role": role,
                "holder": holder,
                "model": None,
                "lineage": None,
                "ownerGrant": None,
            },
            actor=actor,
            scope=scope,
            changed=changed,
            reasons=reasons,
        )

    def active(self, role: str) -> list[JsonValue]:
        return sorted(
            [
                row["holder"]
                for row in record_payloads(self.fixture, "role_list")
                if row.get("role") == role and row.get("ended") is None
            ],
            key=lambda actor: json.dumps(actor, sort_keys=True),
        )

    def recorded_holder(self, role: str, actor: JsonObject) -> JsonObject:
        rows = [
            object_value(row["holder"])
            for row in record_payloads(self.fixture, "role_list")
            if row.get("role") == role
            and row.get("ended") is None
            and row["holder"] == actor
        ]
        if len(rows) != 1:
            raise JourneyFailure(f"expected one active {role} assignment for {actor}")
        return rows[0]


def role_authority_and_cardinality(fixture: BoardFixture) -> None:
    roles = RoleScenario(fixture)
    verifier = human_actor("m2b-verifier")
    reviewer_a, reviewer_b = human_actor("reviewer-a"), human_actor("reviewer-b")
    operator_a, operator_b = human_actor("operator-a"), human_actor("operator-b")
    advisor = human_actor("advisor")
    outsider = human_actor("outsider")
    _ = roles.grant("implementer", fixture.helper)
    _ = roles.grant("verifier", verifier)
    _ = roles.grant(
        "reviewer",
        fixture.helper,
        actor=outsider,
        reasons=[role_required({"kind": "holds", "role": "lead"})],
    )
    for holder in [fixture.helper, verifier]:
        _ = roles.grant(
            "reviewer",
            holder,
            reasons=[
                {
                    "kind": "role",
                    "reason": {"kind": "pairNotDistinct", "holder": holder},
                }
            ],
        )
    _ = roles.grant("reviewer", reviewer_a)
    _ = roles.grant("reviewer", reviewer_b)
    fixture.receipt.equal(
        "Many reviewer grant retains both exact active holders",
        "RL-1/RL-6",
        ([reviewer_a, reviewer_b], roles.active("reviewer")),
    )
    _ = roles.grant("reviewer", reviewer_a, changed=False)
    _ = roles.grant(
        "implementer",
        reviewer_a,
        reasons=[
            {
                "kind": "role",
                "reason": {"kind": "pairNotDistinct", "holder": reviewer_a},
            }
        ],
    )
    _ = roles.grant("advisor", advisor, scope="project")
    _ = roles.grant("operator", operator_a, actor=fixture.helper)
    _ = roles.grant("operator", operator_b)
    fixture.receipt.equal(
        "lead and workstream implementer both grant operators without replacement",
        "RL-7",
        ([operator_a, operator_b], roles.active("operator")),
    )
    _ = roles.grant(
        "operator",
        outsider,
        actor=advisor,
        reasons=[role_required({"kind": "leadOrWorkstreamImplementer"})],
    )
    _ = roles.grant("lead", fixture.owner, scope="project", changed=False)
    _ = roles.grant(
        "lead",
        fixture.owner,
        actor=outsider,
        scope="project",
        reasons=[role_required({"kind": "holds", "role": "lead"})],
    )
    _ = roles.grant(
        "lead",
        outsider,
        scope="project",
        reasons=[{"kind": "invalidShape", "reason": {"field": "role"}}],
    )
    _ = roles.change(
        {"kind": "roleRelease", "role": "reviewer", "holder": None},
        reasons=[{"kind": "invalidShape", "reason": {"field": "holder"}}],
    )
    _ = roles.change(
        {
            "kind": "roleRelease",
            "role": "reviewer",
            "holder": roles.recorded_holder("reviewer", reviewer_a),
        }
    )
    fixture.receipt.equal(
        "Many release ends only its explicitly named holder",
        "RL-2/M2b release",
        ([reviewer_b], roles.active("reviewer")),
    )
    ended: list[JsonValue] = [
        [row["holder"], row["ended"]]
        for row in record_payloads(fixture, "role_list")
        if row.get("role") == "reviewer" and row.get("ended") is not None
    ]
    fixture.receipt.equal(
        "reviewer release leaves append-only exact history",
        "RL-2",
        ([[reviewer_a, {"kind": "released"}]], ended),
    )
    if roles.active("implementer") != [fixture.helper]:
        raise JourneyFailure("refused reverse distinctness grant changed implementer")
    implicit_trail_subject_with_new_roles(fixture, roles)


def implicit_trail_subject_with_new_roles(
    fixture: BoardFixture, roles: RoleScenario
) -> None:
    _ = roles.grant("advisor", fixture.owner, scope="project")
    _ = roles.grant("reviewer", fixture.owner)
    _ = roles.grant("operator", fixture.owner)
    row, text = fixture_identity(), "M2b lead retains its implicit project subject"
    _ = fixture.client.accepted(
        "work_trail_add",
        project_request(
            fixture.owner,
            fixture.project,
            {
                "kind": "workTrailAdd",
                "rowId": row,
                "subject": None,
                "text": text,
                "tags": [],
                "links": [],
            },
        ),
    )
    shown = fixture.client.read(
        "work_trail_show",
        {
            "projectId": fixture.project,
            "reader": {"kind": "anonymous"},
            "selection": {"kind": "forward", "after": {"epoch": 1, "position": 0}},
            "limit": 100,
            "subject": None,
            "tags": [],
            "includeRedacted": False,
        },
    )
    rows = array_value(field(shown, "value", "page", "rows"))
    fixture.receipt.equal(
        "lead with advisor, reviewer and operator assignments stores its implicit Project trail subject",
        "WT-2/M2b role preservation",
        (
            [[row, {"kind": "project"}, text]],
            [
                [
                    field(record, "rowId"),
                    field(record, "subject"),
                    field(record, "text"),
                ]
                for record in rows
            ],
        ),
    )
