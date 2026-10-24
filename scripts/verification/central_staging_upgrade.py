"""Exercise the Router's reqwest adapter, including headerless Unknown fallback.

The local mismatch proxy represents Access only: it signs A's fixture assertion
and forwards B's bearer unchanged to the actual Worker. Its responses originate
at the Worker. The headerless local server is explicitly an adapter component
fixture, not evidence of deployed Cloudflare behavior.
"""

import http.server
import json
import os
import subprocess
import threading
import typing as t
from dataclasses import dataclass

from .board_central_runtime import machine_assertion
from .board_journey_runtime import JourneyReceipt
from .board_journey_values import JsonObject, json_value, object_value
from .central_staging_http import HttpEndpoint, environment_value
from .central_staging_models import LocalTarget, Plan, StagingTarget
from .central_staging_runtime import Routers, require_binary
from .machine_credentials import normalize_machine_credential


@dataclass(frozen=True, kw_only=True)
class ProbeRequest:
    plan: Plan
    endpoint: str
    access_id: str
    access_secret: str
    bearer: str


def probe(request: ProbeRequest) -> JsonObject:
    binary = request.plan.target.native_probe
    require_binary(binary)
    environment = dict(os.environ)
    environment.update(
        {
            "G4_PROBE_ACCESS_ID": request.access_id,
            "G4_PROBE_ACCESS_SECRET": request.access_secret,
            "G4_PROBE_BEARER": request.bearer,
        }
    )
    arguments = {
        "endpoint": request.endpoint,
        "accessIdEnvironment": "G4_PROBE_ACCESS_ID",
        "accessSecretEnvironment": "G4_PROBE_ACCESS_SECRET",
        "credentialEnvironment": "G4_PROBE_BEARER",
    }
    result = subprocess.run(
        [str(binary.path), "upgrade"],
        input=json.dumps(arguments).encode(),
        env=environment,
        capture_output=True,
        check=False,
        timeout=25,
    )
    if result.returncode:
        raise ValueError("actual reqwest probe did not produce an observation")
    return object_value(json_value(result.stdout))


@dataclass(frozen=True, kw_only=True)
class FixtureProxy:
    endpoint: HttpEndpoint | None


@t.final
class LocalRefusal:
    def __init__(self, fixture: FixtureProxy) -> None:
        endpoint = fixture.endpoint
        self.received_headers = False
        owner = self

        class Handler(http.server.BaseHTTPRequestHandler):
            @t.override
            def log_message(self, format: str, *args: object) -> None:
                return

            def do_GET(self) -> None:
                owner.received_headers = all(
                    self.headers.get(name)
                    for name in [
                        "cf-access-client-id",
                        "cf-access-client-secret",
                        "Authorization",
                    ]
                )
                if endpoint is None:
                    self.send_response(401)
                    self.send_header("Content-Length", "0")
                    self.end_headers()
                    return
                forwarded = HttpEndpoint(
                    endpoint.origin,
                    {
                        **endpoint.headers,
                        "Authorization": self.headers.get("Authorization", ""),
                    },
                )
                reply = forwarded.request("GET", "/v1/session")
                self.send_response(reply.status)
                for reason in reply.auth_reasons:
                    self.send_header("fleet-sync-auth-refusal", reason)
                self.send_header("Content-Length", str(len(reply.body)))
                self.end_headers()
                _ = self.wfile.write(reply.body)

        self.server = http.server.ThreadingHTTPServer(("127.0.0.1", 0), Handler)
        self.server.daemon_threads = False
        self.thread = threading.Thread(target=self.server.serve_forever)
        self.thread.start()
        self.endpoint = f"http://127.0.0.1:{self.server.server_port}/v1/session"

    def close(self) -> None:
        self.server.shutdown()
        self.server.server_close()
        self.thread.join(10)
        if self.thread.is_alive():
            raise ValueError("owned refusal fixture did not settle")


@dataclass(frozen=True, kw_only=True)
class UpgradeContext:
    plan: Plan
    routers: Routers
    receipt: JourneyReceipt
    origin: str


def upgrade_observations(context: UpgradeContext) -> None:
    plan, routers, receipt, origin = (
        context.plan,
        context.routers,
        context.receipt,
        context.origin,
    )
    if isinstance(plan.target, LocalTarget):
        assertion = machine_assertion(plan.repository, routers.credentials[0])
        fixtures = [
            (
                FixtureProxy(
                    endpoint=HttpEndpoint(
                        origin, {"Cf-Access-Jwt-Assertion": assertion}
                    )
                ),
                "wrongKind",
            ),
            (FixtureProxy(endpoint=None), "unknown"),
        ]
        for fixture, expected in fixtures:
            server = LocalRefusal(fixture)
            try:
                observed = probe(
                    ProbeRequest(
                        plan=plan,
                        endpoint=server.endpoint,
                        access_id="board-fixture-client",
                        access_secret="board-fixture-secret",
                        bearer="Bearer "
                        + normalize_machine_credential(routers.credentials[1]),
                    )
                )
                receipt.equal(
                    "actual Router reqwest adapter preserves typed refusal/fallback",
                    "OB7 W-C13 component",
                    ({"kind": "credentialRejected", "reason": expected}, observed),
                )
                receipt.equal(
                    "reqwest sends both Access headers and bearer",
                    "OB7 local header generation",
                    (True, server.received_headers),
                )
            finally:
                server.close()
        return
    wrong_kind_request, unknown_request = staging_upgrade_requests(plan)
    observed = probe(wrong_kind_request)
    receipt.equal(
        "deployed typed WrongKind crosses the real reqwest upgrade",
        "OB7 W-C13 / header transit",
        ({"kind": "credentialRejected", "reason": "wrongKind"}, observed),
    )
    observed = probe(unknown_request)
    receipt.equal(
        "Access headerless 401/403 maps to Unknown",
        "OB7 W-C13 fallback on deployed edge",
        ({"kind": "credentialRejected", "reason": "unknown"}, observed),
    )


def staging_upgrade_requests(plan: Plan) -> tuple[ProbeRequest, ProbeRequest]:
    target = plan.target
    if not isinstance(target, StagingTarget):
        raise TypeError("deployed upgrade requests require a staging target")
    first, second = target.installations
    # Cross A's Access identity with B's bearer, then use invalid Access with A.
    return (
        ProbeRequest(
            plan=plan,
            endpoint=target.origin + "/v1/session",
            access_id=environment_value(first.credentials.access_id),
            access_secret=environment_value(first.credentials.access_secret),
            bearer="Bearer "
            + normalize_machine_credential(
                environment_value(second.credentials.machine)
            ),
        ),
        ProbeRequest(
            plan=plan,
            endpoint=target.origin + "/v1/session",
            access_id="invalid-ob7-client",
            access_secret="invalid-ob7-secret",
            bearer="Bearer "
            + normalize_machine_credential(
                environment_value(first.credentials.machine)
            ),
        ),
    )
