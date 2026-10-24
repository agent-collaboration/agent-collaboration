"""Public OB4 operations and per-case coverage, preserving unresolved obligations."""

import datetime
import hashlib
import json
import os
import time

from .board_central_runtime import FixtureAccessEdge, RealCentral
from .board_journey_runtime import JourneyReceipt, project_request, write_header
from .board_journey_values import (
    JourneyFailure,
    JsonObject,
    JsonValue,
    array_value,
    field,
    fixture_identity,
    json_value,
    object_value,
)
from .board_store_readback import read_stopped_store
from .cross_machine_central import MatrixCentral
from .cross_machine_lifecycle import Installation, SessionControl
from .cross_machine_plan import BinaryBinding, CentralSession, CredentialNames
from .cross_machine_seed import import_unheld

CASES = {
    "fleet-central-unreachable": "Original fleet request remains pending while central is down, then is decided once",
    "forwarding": "Verified-origin root/reply, replay, offline/unheld/deadline/cap and no stored forwarding content",
    "both-directions": "Messages and notifications observed in actual recipient sessions in both directions",
    "local-independence": "Actual local recipient receives while the central session is paused",
    "held-copy-ack": "Unrelated mail arrives during copy-ack-only hold; remote notification arrives once after release",
    "distinct-handoffs": "One cause freezes distinct stable item IDs per local/remote recipient",
    "receipt-retention": "After retirement original submission replay causes no new delivery and preserves final sender status; compact row is separate component proof (1100)",
    "offline-retry": "Actual recipient reconnect and retries cause no second injection",
    "cancellation": "Recipient injection decides prevention/too-late, preserving board replication/generation eligibility",
    "wakes": "Real wake reaches declared recipient; slot/creator/lifetime rules hold",
}


def await_accepted(
    installation: Installation, tool: str, request: JsonObject
) -> JsonObject:
    deadline = time.monotonic() + 30
    while True:
        answer = installation.read(tool, request)
        kind = field(answer, "result", "kind")
        if kind not in {"pendingUnreachable", "projectCreationPending"}:
            if field(answer, "result", "decision", "kind") != "accepted":
                raise JourneyFailure("expected accepted public " + tool)
            return answer
        if time.monotonic() >= deadline:
            raise JourneyFailure("same-ID public request never settled")
        time.sleep(0.2)


def enroll_installation(
    installation: Installation, central: RealCentral
) -> FixtureAccessEdge:
    identity = installation.read("installation_show", {})
    installation.close()
    credential = "ob4-fixture-" + fixture_identity()
    digest = hashlib.sha256(credential.encode()).hexdigest()
    answer = central.post(
        "/admin/v1/machines/enroll",
        {
            "kind": "enrollMachine",
            "value": {
                "label": identity["machineLabel"],
                "installation": identity["serviceId"],
                "peer": identity["peerId"],
                "access_client_id": "test-client-" + digest + ".access",
                "credential_hash": digest,
                "expires_at": "2033-05-18T03:33:20Z",
            },
        },
    )
    if answer.get("kind") != "machineEnrolled":
        raise JourneyFailure("local fixture enrollment refused")
    edge = FixtureAccessEdge(central.plan.repository, central.ready.url, credential)
    prefix = "OB4_" + installation.plan.name.upper().replace("-", "_")
    names = CredentialNames(
        machine=prefix + "_MACHINE",
        access_id=prefix + "_ACCESS_ID",
        access_secret=prefix + "_ACCESS_SECRET",
    )
    environment = dict(os.environ)
    environment.update(
        {
            names.machine: "Bearer " + credential,
            names.access_id: "board-fixture-client",
            names.access_secret: "board-fixture-secret",
        }
    )
    installation.environment = environment
    installation.plan = installation.plan.model_copy(
        update={
            "session": CentralSession(
                endpoint=edge.endpoint,
                credentials=names,
                peer=str(identity["peerId"]),
                receiver=str(field(answer, "value", "authoritativeReceiver")),
                relay=str(field(answer, "value", "relayPeer")),
            )
        }
    )
    installation.start()
    return edge


def fleet_outage(
    first: Installation, edge: SessionControl, receipt: JourneyReceipt
) -> JsonObject:
    actor: JsonObject = {"kind": "human", "humanId": "m2-verification-lead"}
    workspace = fixture_identity()
    request: JsonObject = {
        **write_header(actor),
        "workspaceId": workspace,
        "name": workspace,
        "description": "OB4 original-ID outage",
    }
    # Establish an actual connected operation before deliberately disconnecting.
    _ = await_accepted(
        first,
        "workspace_create",
        {
            **write_header(actor),
            "workspaceId": fixture_identity(),
            "name": fixture_identity(),
            "description": "owned preflight",
        },
    )
    edge.pause()
    try:
        deadline = time.monotonic() + 10
        while True:
            answer = first.read("workspace_create", request)
            if field(answer, "result", "kind") == "pendingUnreachable":
                break
            if time.monotonic() >= deadline:
                raise JourneyFailure(
                    "disconnected fixture did not become pendingUnreachable"
                )
            time.sleep(0.1)
        receipt.equal(
            "outage retains original undecided request",
            "OB4 / WA-1",
            ("pendingUnreachable", field(answer, "result", "kind")),
        )
        service_id = first.manifest["serviceId"]
        first.close()
        try:
            service = first.plan.task_directory / "service"
            cache = read_stopped_store(service / "fleet-cache.sqlite")
            outbox = read_stopped_store(service / "fleet-outbox.sqlite")
            cached = [
                row
                for row in cache.tables["cached_fleet_records"].rows
                if row.get("id") == workspace
            ]
            pending = [
                row
                for row in outbox.tables["fleet_outbox_requests"].rows
                if row.get("request_id") == request["requestId"]
            ]
            receipt.equal(
                "outage creates no cached authoritative workspace",
                "WA-1",
                (0, len(cached)),
            )
            receipt.equal(
                "outage persists the original request only as pending",
                "WA-1 / RC-4",
                (
                    [1, "pending"],
                    [
                        len(pending),
                        field(json_value(str(pending[0]["state"])), "kind")
                        if pending
                        else None,
                    ],
                ),
            )
            receipt.equal(
                "Session composition creates no local authoritative fleet store",
                "WA-1",
                (False, (service / "fleet.sqlite").exists()),
            )
            receipt.record(
                {
                    "kind": "outageStoppedStoreReadback",
                    "cacheDigests": dict[str, JsonValue](cache.file_digests),
                    "outboxDigests": dict[str, JsonValue](outbox.file_digests),
                    "requestId": request["requestId"],
                }
            )
        finally:
            first.start()
        receipt.equal(
            "pending fleet restart keeps installation identity",
            "WA-1",
            (service_id, first.manifest["serviceId"]),
        )
    finally:
        edge.resume()
    admitted = await_accepted(first, "workspace_create", request)
    replay = first.read("workspace_create", request)
    receipt.equal(
        "same-ID fleet retry keeps exact first decision",
        "OB4 / RC-4",
        (admitted["result"], replay["result"]),
    )
    return {"actor": actor, "workspace": workspace}


def forwarding(
    first: Installation,
    second: Installation,
    central: MatrixCentral,
    context: JsonObject,
    receipt: JourneyReceipt,
    importer: BinaryBinding | None,
) -> JsonObject:
    actor = object_value(context["actor"])
    board, project, post, reply_id = [fixture_identity() for _ in range(4)]
    _ = await_accepted(
        first,
        "board_create",
        {
            **write_header(actor),
            "workspaceId": context["workspace"],
            "boardId": board,
            "name": board,
            "description": "owned OB4 board",
            "kind": {"kind": "area"},
        },
    )
    _ = await_accepted(
        first,
        "project_create",
        {
            **write_header(actor),
            "boardId": board,
            "projectId": project,
            "title": "OB4 public forwarding",
            "description": "owned observation",
        },
    )
    _ = await_accepted(
        first,
        "role_grant",
        project_request(
            actor,
            project,
            {
                "kind": "roleGrant",
                "subject": {"kind": "project", "subject": project},
                "role": "lead",
                "holder": actor,
                "model": None,
                "lineage": None,
                "expectedRevision": 0,
                "ownerGrant": None,
            },
        ),
    )
    root: JsonObject = {
        **write_header(actor),
        "postId": post,
        "projectId": project,
        "about": {"kind": "project", "subject": {"line": "OB4 forwarded root"}},
        "text": "forwarded root " + post,
        "references": [],
        "addressees": [],
        "role": None,
        "watch": False,
    }
    created = await_accepted(second, "thread_create", root)
    reply: JsonObject = {
        **write_header(actor),
        "postId": reply_id,
        "placement": {"kind": "thread", "thread": {"project": project, "thread": post}},
        "text": "forwarded reply " + reply_id,
        "references": [],
        "addressees": [],
    }
    posted = await_accepted(second, "thread_post", reply)
    lane = fixture_identity()
    _ = await_accepted(
        first,
        "workstream_create",
        project_request(
            actor,
            project,
            {
                "kind": "workstreamCreate",
                "workstreamId": lane,
                "name": "OB4 non-post boundary",
                "goal": "Refuse task forwarding",
            },
        ),
    )
    before = first.read("view_work", {"projectId": project, "includeRemoved": False})
    task = project_request(
        actor,
        project,
        {
            "kind": "taskCreate",
            "taskId": fixture_identity(),
            "workstreamId": lane,
            "title": "must not forward",
            "body": "Owned OB4 negative observation",
            "blockedBy": [],
            "relatedTo": [],
        },
    )
    refused_task = second.read("task_create", task)
    receipt.record({"kind": "nonPostRefusal", "reply": refused_task})
    receipt.equal(
        "uncopied non-post refusal reports unknown epoch",
        "WA-2 / decision1110",
        (
            ["notWriter", None],
            [
                field(refused_task, "result", "kind"),
                field(refused_task, "result", "decision", "current"),
            ],
        ),
    )
    for tool, request, original in [
        ("thread_create", root, created),
        ("thread_post", reply, posted),
    ]:
        retry = second.read(tool, request)
        receipt.equal(
            tool + " replays the holder decision",
            "OB4 / WA-13",
            (original["result"], retry["result"]),
        )
    receipt.equal(
        "forwarded retry appends no feed effect",
        "OB4 / MS-2",
        (
            before["asOf"],
            first.read("view_work", {"projectId": project, "includeRemoved": False})[
                "asOf"
            ],
        ),
    )
    text = second.plan.task_directory / "forwarded-root.txt"
    _ = text.write_text(str(root["text"]))
    cli = second.cli(
        [
            "thread",
            "create",
            "--project-id",
            project,
            "--post-id",
            post,
            "--request-id",
            str(root["requestId"]),
            "--about",
            "project:OB4 forwarded root",
            "--actor",
            json.dumps(actor),
            "--no-watch",
            "--text-file",
            str(text),
        ]
    )
    receipt.equal(
        "CLI repeats the same semantic post and exact holder decision",
        "OB4 CLI/MCP",
        (
            created["result"],
            field(cli, "result", "record", "result"),
        ),
    )
    feed = first.read(
        "feed_read",
        {
            "projectId": project,
            "reader": {"kind": "anonymous"},
            "after": {"epoch": 1, "position": 0},
            "limit": 100,
            "includeRedacted": False,
        },
    )
    for request in [root, reply]:
        found = [
            object_value(entry)
            for entry in array_value(field(feed, "value", "entries"))
            if field(entry, "record") == {"kind": "post", "id": request["postId"]}
        ]
        receipt.equal("one typed forwarded Post", "OB4", (1, len(found)))
        receipt.equal(
            "origin is enrolled sending installation",
            "FL-14",
            (second.manifest["serviceId"], field(found[0], "origin", "installation")),
        )
        receipt.equal(
            "declared actor survives forwarding", "FL-14", (actor, found[0]["actor"])
        )
    if importer is not None:
        unheld = import_unheld(first, central, importer, receipt)
        refused_unheld = second.read(
            "thread_create",
            {
                **root,
                "requestId": fixture_identity(),
                "postId": fixture_identity(),
                "projectId": unheld,
            },
        )
        receipt.equal(
            "imported unheld project refuses forwarding",
            "FL-14/15",
            (
                "notHeld",
                field(refused_unheld, "result", "kind"),
            ),
        )
    else:
        receipt.record(
            {
                "kind": "gap",
                "case": "forwarding",
                "detail": "No declared importer binding for synthetic unheld project.",
            }
        )
    deadline = time.monotonic() + 30
    while True:
        copied = second.read(
            "view_work", {"projectId": project, "includeRemoved": False}
        )
        if copied.get("asOf") == before["asOf"]:
            break
        if time.monotonic() >= deadline:
            raise JourneyFailure(
                "B's public read did not acquire the completed authority"
            )
        time.sleep(0.2)
    receipt.equal(
        "nonholder actually installed a central read copy",
        "WA-2 prerequisite",
        (
            {"kind": "readCopy"},
            copied.get("source"),
        ),
    )
    known_refusal = second.read(
        "task_create", {**task, "requestId": fixture_identity()}
    )
    receipt.equal(
        "copied non-post refusal names the known epoch",
        "WA-2 / decision1110",
        (
            ["notWriter", 1],
            [
                field(known_refusal, "result", "kind"),
                field(known_refusal, "result", "decision", "current"),
            ],
        ),
    )
    receipt.equal(
        "non-post refusals write no holder feed effect",
        "WA-2",
        (
            before["asOf"],
            first.read("view_work", {"projectId": project, "includeRemoved": False})[
                "asOf"
            ],
        ),
    )
    first.close()
    preview = central.post(f"/admin/v1/projects/{project}/takeover/preview", None)
    refused = second.read(
        "thread_post",
        {**reply, "requestId": fixture_identity(), "postId": fixture_identity()},
    )
    receipt.equal(
        "offline holder is refused explicitly",
        "FL-14",
        ("holderUnreachable", field(refused, "result", "kind")),
    )
    receipt.record(
        {"kind": "offlineHolder", "independentPresence": preview, "reply": refused}
    )
    last_seen = preview.get("lastSeen")
    observed = field(refused, "result", "decision", "lastSeen")
    if not isinstance(last_seen, int) or not isinstance(observed, str):
        raise JourneyFailure("independent offline lastSeen is absent")
    receipt.equal(
        "offline refusal preserves independently observed lastSeen",
        "FL-14",
        (
            last_seen,
            int(datetime.datetime.fromisoformat(observed).timestamp() * 1000),
        ),
    )
    first.start()
    return {"project": project, "actor": actor, "workspace": context["workspace"]}


def recipient_gaps(
    installations: list[Installation], receipt: JourneyReceipt
) -> dict[str, str]:
    inventories = [item.read("endpoints_list", {}) for item in installations]
    receipt.record(
        {"kind": "recipientEndpointInventories", "values": list[JsonValue](inventories)}
    )
    reason = (
        "Actual recipient model/authentication and case-specific observation are required. A provider-enabled launcher alone does not prove arrival; no stand-in was substituted."
        if all(item.plan.recipient_provider is not None for item in installations)
        else "Declared launcher lacks a real provider recipient binding; actual arrival remains unobserved."
    )
    controls = {
        "both-directions": "Actual message and notification observations both ways are pending.",
        "local-independence": "Actual local notification during Session pause is pending.",
        "held-copy-ack": "The existing pre-apply Copy dispatch hold is coded; actual remote notification withholding plus unrelated model arrival awaits authenticated recipients and target qualification.",
        "distinct-handoffs": "Public per-recipient IDs and arrivals are pending; durable handoff-body reopen also remains unqualified.",
        "receipt-retention": "Actual retirement/unsettled-restart replay is unqualified; compact-row proof stays at the Tokio component boundary.",
        "offline-retry": "Session reconnect/same-ID observation is pending; receipt retention drives unsettled sender recovery. Provider restart and long-term dedup remain unqualified.",
        "cancellation": "Actual post-injection tooLate is pending; prevention/membership/generation/copy controls remain unqualified.",
        "wakes": "Actual timer/slot observation is pending; target replacement and timing/lifetime matrix remain unqualified.",
    }
    return {name: reason + " " + detail for name, detail in controls.items()}
