"""OB7 assertions against actual Router views and the owning Worker HTTP faces."""

import subprocess
import time
import typing as t
from concurrent.futures import ThreadPoolExecutor
from dataclasses import dataclass

from .board_central_cases import await_view, await_writer, work_view
from .board_central_runtime import machine_assertion
from .board_journey_fixture import BoardFixture, accepted_decision
from .board_journey_runtime import BoardClient, JourneyReceipt
from .board_journey_values import (
    JsonObject,
    JsonValue,
    array_value,
    field,
    file_digest,
    fixture_identity,
    json_value,
    object_value,
)
from .central_staging_http import HttpEndpoint, environment_value
from .central_staging_models import AddressInventory, LocalTarget, Plan, StagingTarget
from .central_staging_runtime import LocalWorker, Routers, require_binary, session_state
from .central_staging_upgrade import UpgradeContext, upgrade_observations
from .machine_credentials import normalize_machine_credential


@dataclass(frozen=True, kw_only=True)
class CaseContext:
    plan: Plan
    receipt: JourneyReceipt
    owner: HttpEndpoint
    routers: Routers
    fixture: BoardFixture
    local: LocalWorker | None


def copy_status(context: CaseContext) -> JsonObject:
    health = context.owner.get(f"/admin/v1/copies/{context.fixture.project}/health")
    context.receipt.record({"kind": "copyHealth", "value": health})
    if (
        health.get("kind") != "live"
        or field(health, "value", "copy", "kind") != "known"
    ):
        raise ValueError("owning Copy health is not known/live")
    return object_value(field(health, "value", "copy", "value"))


def session_ids(context: CaseContext) -> list[JsonValue]:
    return [
        str(session_state(runtime).sessionId) for runtime in context.routers.runtimes
    ]


def deploy(context: CaseContext) -> None:
    if context.local is not None:
        if context.local.child is None or context.local.child.poll() is not None:
            raise ValueError("the selected local Worker was not live before restart")
        previous = context.local.child.pid
        context.local.restart()
        observed = context.local.child.pid
        context.receipt.equal(
            "local recovery actually changes the owned process",
            "OB7 local process boundary",
            (True, observed != previous),
        )
        return
    target = context.plan.target
    if not isinstance(target, StagingTarget) or target.deploy is None:
        raise ValueError("explicit staging deploy binding is absent")
    binding = target.deploy
    require_binary(binding.executable)
    for source in binding.source_files:
        require_binary(source)
    before = file_digest(binding.namespace_pin)
    log = context.receipt.directory / f"deploy-{fixture_identity()}.log"
    with log.open("xb") as output:
        result = subprocess.run(
            [str(binding.executable.path), *binding.arguments],
            cwd=binding.cwd,
            stdout=output,
            stderr=subprocess.STDOUT,
            check=False,
            timeout=600,
        )
    context.receipt.equal(
        "guarded deploy exits successfully",
        "OB7 deploy durability",
        (0, result.returncode),
    )
    context.receipt.equal(
        "deploy preserves the pinned namespace bytes",
        "OB7 namespace continuity",
        (before, file_digest(binding.namespace_pin)),
    )
    context.receipt.record(
        {
            "kind": "deploy",
            "executableSha256": binding.executable.sha256,
            "log": str(log),
            "namespacePinSha256": before,
        }
    )


def read_copy_contains(context: CaseContext, marker: str, post: str) -> None:
    fixture = context.fixture
    reader = BoardClient(context.routers.runtimes[1])
    expected = work_view(fixture.client, fixture.project)
    observed = await_view(reader, fixture.project, expected["asOf"])
    context.receipt.equal(
        "reader uses actual central Copy",
        "OB7 two Router readback",
        ({"kind": "readCopy"}, observed.get("source")),
    )
    context.receipt.equal(
        "reader sees the same acknowledged work lanes",
        "OB7 replication",
        (expected.get("workstreams"), observed.get("workstreams")),
    )
    feed = reader.read(
        "feed_read",
        {
            "projectId": fixture.project,
            "reader": {"kind": "anonymous"},
            "after": {"epoch": 1, "position": 0},
            "limit": 100,
            "includeRedacted": False,
        },
    )
    entries = [
        object_value(row)
        for row in array_value(field(feed, "value", "entries"))
        if field(row, "record", "id") == post
    ]
    context.receipt.equal(
        "one exact immutable replicated post",
        "OB7 no lost/duplicate effect",
        (1, len(entries)),
    )
    context.receipt.equal(
        "replicated post retains exact marker bytes",
        "OB7 data continuity",
        (marker, field(entries[0], "content", "record", "record", "text")),
    )
    context.receipt.record(
        {
            "kind": "copyReadback",
            "holder": expected,
            "reader": observed,
            "posts": list[JsonValue](entries),
        }
    )


def replication_deploy(context: CaseContext) -> None:
    fixture = context.fixture
    thread = fixture.create_thread()
    fixture.wait_replicated()
    connection: JsonObject = {
        "kind": "connection",
        "value": context.routers.identities[0].serviceId,
    }
    copy: JsonObject = {"kind": "copy", "value": fixture.project}
    _ = context.owner.control(
        connection, {"kind": "holdNextDispatch", "value": {"destination": copy}}
    )
    try:
        marker = "OB7 in-flight replication " + fixture_identity()
        request = fixture.reply_request(thread, marker)
        _ = accepted_decision(fixture.client.accepted("thread_post", request))
        held = context.owner.control(connection, {"kind": "awaitHeldDispatch"})
        context.receipt.equal(
            "Copy-bound dispatch is held before deploy",
            "OB7 deploy during replication",
            ("held", field(held, "dispatch", "kind")),
        )
        opaque = array_value(field(held, "dispatch", "value", "request"))
        if any(type(value) is not int or not 0 <= value <= 255 for value in opaque):
            raise ValueError("held dispatch did not retain bytes")
        envelope = object_value(json_value(bytes(t.cast(list[int], opaque))))
        request_kind = field(envelope, "request", "request", "kind")
        context.receipt.equal(
            "held dispatch is an actual replication Piece",
            "OB7 deciding hold boundary",
            ("piece", request_kind),
        )
        context.receipt.equal(
            "held Piece addresses the intended project",
            "OB7 deciding hold boundary",
            (
                fixture.project,
                field(envelope, "request", "request", "value", "database"),
            ),
        )
        before = session_ids(context)
        context.receipt.record(
            {
                "kind": "replicationBeforeDeploy",
                "sessions": before,
                "dispatch": held,
                "copy": copy_status(context),
            }
        )
        deploy(context)
        fixture.wait_replicated()
        read_copy_contains(context, marker, str(request["postId"]))
        context.receipt.equal(
            "both Router session IDs survive deployment",
            "OB7 same-session resume",
            (before, session_ids(context)),
        )
        replay = fixture.client.read("thread_post", request)
        context.receipt.equal(
            "exact post retry remains accepted",
            "OB7 replay after deploy",
            ("accepted", accepted_decision(replay).get("kind")),
        )
        read_copy_contains(context, marker, str(request["postId"]))
    finally:
        try:
            _ = context.owner.control(connection, {"kind": "resumeDispatch"})
        except (OSError, ValueError):
            # A redeploy discards the old ephemeral hold; an early failure must
            # still attempt to release a live hold before owned Host cleanup.
            pass


def takeover_recovery(context: CaseContext) -> None:
    fixture = context.fixture
    thread = fixture.create_thread()
    fixture.wait_replicated()
    original_view = work_view(fixture.client, fixture.project)
    previous_copy = copy_status(context)
    preview = context.owner.post(
        f"/admin/v1/projects/{fixture.project}/takeover/preview", None
    )
    native = object_value(preview["preview"])
    context.receipt.equal(
        "preview holder is the actual first Router",
        "OB7 holder identity",
        (context.routers.identities[0].serviceId, preview.get("holderInstallation")),
    )
    fence = native["fence"]
    if not isinstance(fence, int):
        raise TypeError("native preview fence is not an epoch")
    request: JsonObject = {
        "transferId": fixture_identity(),
        "toInstallation": context.routers.identities[1].serviceId,
        "reason": "OB7 interrupted owner takeover",
        "preview": native,
    }
    registry: JsonObject = {"kind": "registry"}
    _ = context.owner.control(
        registry,
        {
            "kind": "dropNextObjectReply",
            "value": {"destination": {"kind": "copy", "value": fixture.project}},
        },
    )
    route = f"/admin/v1/projects/{fixture.project}/takeover"
    with ThreadPoolExecutor(max_workers=1) as executor:
        pending = executor.submit(context.owner.post, route, request)
        dropped = context.owner.control(registry, {"kind": "awaitDroppedObjectReply"})
        context.receipt.equal(
            "Copy fence reply was actually lost",
            "OB7 interrupted fence",
            ("dropped", field(dropped, "objectReply", "kind")),
        )
        fenced = copy_status(context)
        context.receipt.equal(
            "Copy fence advances before the missing reply is recovered",
            "OB7 fence ordering",
            (fence + 1, fenced.get("fence")),
        )
        context.receipt.equal(
            "fencing retains the independently read acknowledged prefix",
            "OB7 no invented head",
            (previous_copy.get("acked"), fenced.get("acked")),
        )
        deploy(context)
        try:
            _ = pending.result(timeout=60)
        except (OSError, ValueError):
            # The deploy can sever this HTTP connection after the Copy committed.
            pass
    deadline = time.monotonic() + 90
    answer: JsonObject = {}
    while time.monotonic() < deadline:
        answer = context.owner.post(route, request)
        if answer.get("kind") != "pending":
            break
        time.sleep(0.5)
    expected: JsonObject = {
        "kind": "granted",
        "value": {"newEpoch": fence + 1, "start": native["acked"]},
    }
    context.receipt.equal(
        "recovered takeover grants one exact next epoch and prefix",
        "OB7 fence/takeover recovery",
        (expected, answer),
    )
    context.receipt.equal(
        "same takeover identity replays its exact answer",
        "OB7 one grant",
        (answer, context.owner.post(route, request)),
    )
    after = copy_status(context)
    context.receipt.equal(
        "Copy names the target peer after recovery",
        "OB7 physical Copy owner readback",
        (context.routers.identities[1].peerId, after.get("holder")),
    )
    target = BoardClient(context.routers.runtimes[1])
    writing = await_writer(target, fixture.project, fence + 1)
    context.receipt.equal(
        "successor installs the original acknowledged lanes before use",
        "OB7 usable takeover",
        (original_view["workstreams"], writing["workstreams"]),
    )
    marker = "OB7 successor write " + fixture_identity()
    posted = fixture.reply_request(thread, marker)
    _ = accepted_decision(target.accepted("thread_post", posted))
    deadline = time.monotonic() + 30
    while time.monotonic() < deadline:
        writing = work_view(target, fixture.project)
        if writing.get("asOf") == writing.get("replicatedThrough"):
            break
        time.sleep(0.2)
    else:
        raise ValueError("successor's real post did not reach the authoritative Copy")
    old_reader = BoardClient(context.routers.runtimes[0])
    copied = await_view(old_reader, fixture.project, writing["asOf"])
    context.receipt.equal(
        "former holder reads the successor's actual Copy",
        "OB7 usable takeover",
        ({"kind": "readCopy"}, copied.get("source")),
    )
    as_of = object_value(writing["asOf"])
    feed = old_reader.read(
        "feed_read",
        {
            "projectId": fixture.project,
            "reader": {"kind": "anonymous"},
            "after": {"epoch": as_of["epoch"], "position": 0},
            "limit": 100,
            "includeRedacted": False,
        },
    )
    entries = [
        object_value(row)
        for row in array_value(field(feed, "value", "entries"))
        if field(row, "record", "id") == posted["postId"]
    ]
    context.receipt.equal(
        "successor post is copied exactly once",
        "OB7 usable takeover",
        (1, len(entries)),
    )
    context.receipt.equal(
        "successor post preserves exact bytes",
        "OB7 usable takeover",
        (marker, field(entries[0], "content", "record", "record", "text")),
    )
    context.receipt.equal(
        "successor post carries the target's enrolled origin",
        "OB7 usable takeover",
        (
            context.routers.identities[1].serviceId,
            field(entries[0], "origin", "installation"),
        ),
    )
    directory = context.owner.get("/admin/v1/copies?limit=100")
    context.receipt.record(
        {
            "kind": "takeoverAfter",
            "answer": answer,
            "copy": after,
            "directory": directory,
        }
    )


def idle_resume(context: CaseContext) -> None:
    fixture = context.fixture
    fixture.wait_replicated()
    before = session_ids(context)
    copy_before = copy_status(context)
    started = time.monotonic()
    time.sleep(context.plan.idle_seconds)
    context.receipt.record(
        {
            "kind": "idleWindow",
            "elapsedSeconds": time.monotonic() - started,
            "evictionEvidenced": False,
            "gap": "No existing object construction/startup signal; request invocation IDs do not establish eviction.",
        }
    )
    thread = fixture.create_thread()
    marker = "OB7 after idle " + fixture_identity()
    request = fixture.reply_request(thread, marker)
    _ = accepted_decision(fixture.client.accepted("thread_post", request))
    fixture.wait_replicated()
    read_copy_contains(context, marker, str(request["postId"]))
    context.receipt.equal(
        "session identity survives the idle window",
        "OB7 idle/resume survival only",
        (before, session_ids(context)),
    )
    context.receipt.equal(
        "idle leaves Copy fence unchanged",
        "OB7 durable state survival",
        (copy_before.get("fence"), copy_status(context).get("fence")),
    )


def machine_headers(target: StagingTarget, index: int) -> dict[str, str]:
    credentials = target.installations[index].credentials
    return {
        "cf-access-client-id": environment_value(credentials.access_id),
        "cf-access-client-secret": environment_value(credentials.access_secret),
        "Authorization": "Bearer "
        + normalize_machine_credential(environment_value(credentials.machine)),
    }


def access(context: CaseContext) -> None:
    thread = context.fixture.create_thread()
    marker = "OB7 header transit " + fixture_identity()
    request = context.fixture.reply_request(thread, marker)
    _ = accepted_decision(context.fixture.client.accepted("thread_post", request))
    context.fixture.wait_replicated()
    read_copy_contains(context, marker, str(request["postId"]))
    upgrade_observations(
        UpgradeContext(
            plan=context.plan,
            routers=context.routers,
            receipt=context.receipt,
            origin=context.owner.origin,
        )
    )
    # A successful owner operations read reaches the real signature-checking gate.
    _ = context.owner.get("/admin/v1/copies?limit=1")
    if isinstance(context.plan.target, LocalTarget):
        if context.local is None:
            raise ValueError("local fixture missing")
        valid = (context.local.fixture_root / "owner.jwt").read_text()
        forged = (context.local.fixture_root / "forged.jwt").read_text()
        verifier_cases: list[tuple[str, JsonObject]] = [
            (valid, {"kind": "accepted", "value": "owner"}),
            (forged, {"kind": "refused", "value": {"kind": "badSignature"}}),
        ]
        for token, expected in verifier_cases:
            reply = context.owner.post(
                "/admin/v1/conformance/access",
                {"assertion": token, "path": "/admin/v1/copies"},
            )
            context.receipt.equal(
                "actual Worker verifier accepts signed bytes and rejects a decoded-only forgery",
                "OB7 local verifier component",
                (expected, reply),
            )
        machine = HttpEndpoint(
            context.local.origin,
            {
                "Cf-Access-Jwt-Assertion": machine_assertion(
                    context.plan.repository, context.routers.credentials[0]
                ),
                "Authorization": "Bearer "
                + normalize_machine_credential(context.routers.credentials[0]),
            },
        )
        refused = machine.request("GET", "/admin/v1/copies?limit=1")
        context.receipt.equal(
            "machine cannot read owner admin",
            "OB7 machine/admin separation",
            (True, refused.status in {401, 403}),
        )
        context.receipt.record(
            {
                "kind": "localAccessLimit",
                "stagingAccessPolicy": "unobserved",
                "alternatePublicAddresses": "unobserved",
            }
        )
        return
    target = context.plan.target
    inventory = AddressInventory.model_validate_json(
        target.address_inventory.read_bytes()
    )
    for source in inventory.inventory_sources:
        require_binary(source)
    context.receipt.equal(
        "all inventory alternate origins are selected",
        "OB7 declared address closure",
        (
            list[JsonValue](sorted(inventory.alternate_origins)),
            list[JsonValue](sorted(target.alternate_origins)),
        ),
    )
    context.receipt.equal(
        "address inventory names this deployed script and version",
        "OB7 target identity",
        (
            [target.script, target.deployed_version],
            [inventory.script, inventory.deployed_version],
        ),
    )
    for number, origin in enumerate(target.alternate_origins):
        for path in ["/v1/session", "/admin/v1/copies?limit=1"]:
            reply = HttpEndpoint(origin, machine_headers(target, 0)).request(
                "GET", path
            )
            context.receipt.equal(
                f"alternate address {number} refuses {path}",
                "OB7 no Access bypass",
                (True, reply.status in {401, 403, 404, 410}),
            )
    for headers in [{}, {"cf-access-token": "invalid-ob7-assertion"}]:
        refused = HttpEndpoint(target.origin, headers).request(
            "GET", "/admin/v1/copies?limit=1"
        )
        context.receipt.equal(
            "missing/invalid Access cannot reach owner data",
            "OB7 real edge denial",
            (True, refused.status in {301, 302, 303, 307, 308, 401, 403}),
        )
    refused = HttpEndpoint(target.origin, machine_headers(target, 0)).request(
        "GET", "/admin/v1/copies?limit=1"
    )
    context.receipt.equal(
        "real service token plus machine bearer is refused on admin",
        "OB7 deployed machine/admin separation",
        (True, refused.status in {401, 403}),
    )
    context.receipt.record(
        {
            "kind": "stagingAccessLimit",
            "decodedOnlyWorkerNegative": "local component only (1115)",
            "headersTransit": "two real Router sessions and actual replicated Copy readback, plus typed mismatch probe",
        }
    )
