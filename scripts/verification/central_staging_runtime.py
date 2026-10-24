"""Own the local workerd/JWKS children and two real isolated Router lifetimes."""

import hashlib
import os
import selectors
import shutil
import signal
import socket
import subprocess
import time
import typing as t
from dataclasses import dataclass

from .board_central_runtime import FixtureAccessEdge, InstallationIdentity, private_json
from .board_journey_runtime import BoardClient, BoardRuntime, JourneyReceipt
from .board_journey_target import JourneyTarget
from .board_journey_values import (
    JsonObject,
    field,
    file_digest,
    fixture_identity,
    json_value,
    object_value,
    string_value,
)
from .central_runtime import PackageVersion, WorkerdBinding, file_evidence
from .central_staging_http import HttpEndpoint, environment_value
from .central_staging_models import (
    Binary,
    LocalTarget,
    Plan,
    SessionState,
    StagingTarget,
)
from .machine_credentials import normalize_machine_credential


def require_binary(binary: Binary) -> None:
    if (
        binary.path.resolve(strict=True) != binary.path
        or file_digest(binary.path) != binary.sha256
    ):
        raise ValueError("declared executable is absent, noncanonical or changed")


def child_reply(child: subprocess.Popen[bytes]) -> JsonObject:
    if child.stdout is None:
        raise ValueError("owned child has no stdout")
    with selectors.DefaultSelector() as selector:
        _ = selector.register(child.stdout, selectors.EVENT_READ)
        if not selector.select(60):
            raise ValueError(
                "owned child did not publish readiness within sixty seconds"
            )
        return object_value(json_value(t.cast(bytes, child.stdout.readline())))


def stop_child(child: subprocess.Popen[bytes]) -> int:
    if child.poll() is None:
        child.terminate()
        try:
            _ = child.wait(30)
        except subprocess.TimeoutExpired:
            child.kill()
            _ = child.wait(10)
    if child.stdout is not None:
        child.stdout.close()
    if child.stdin is not None:
        child.stdin.close()
    return child.returncode


def inspect_workerd(target: LocalTarget, receipt: JourneyReceipt) -> None:
    package = target.miniflare.resolve(strict=True).parents[2] / "package.json"
    version = PackageVersion.model_validate_json(package.read_bytes())
    if (version.name, version.version) != ("miniflare", "5.20261012.0-beta"):
        raise ValueError("local qualification requires the pinned Miniflare version")
    node = subprocess.run(
        [str(target.node), "--version"], capture_output=True, check=True, timeout=15
    )
    resolved = subprocess.run(
        [
            str(target.node),
            "-e",
            "const r=require('node:module').createRequire(process.argv[1]);const w=r('workerd');console.log(JSON.stringify({version:w.version,executable:w.default}));",
            str(target.miniflare.resolve()),
        ],
        capture_output=True,
        check=True,
        timeout=15,
    )
    binding = WorkerdBinding.model_validate_json(resolved.stdout)
    if binding.version != "1.20261012.1":
        raise ValueError("local qualification requires the pinned workerd version")
    runtime = subprocess.run(
        [str(binding.executable), "--version"],
        capture_output=True,
        check=True,
        timeout=15,
    )
    receipt.record(
        {
            "kind": "workerdTools",
            "nodeVersion": node.stdout.decode().strip(),
            "workerdVersion": runtime.stdout.decode().strip(),
            "miniflareVersion": version.version,
            "tools": [
                object_value(json_value(file_evidence(path).model_dump_json()))
                for path in [
                    target.node,
                    target.miniflare,
                    package,
                    binding.executable,
                ]
            ],
        }
    )


@t.final
class LocalWorker:
    def __init__(self, plan: Plan, receipt: JourneyReceipt) -> None:
        if not isinstance(plan.target, LocalTarget):
            raise TypeError("local workerd target required")
        self.plan, self.receipt, self.target = plan, receipt, plan.target
        self.child: subprocess.Popen[bytes] | None = None
        self.logs: list[t.BinaryIO] = []
        self.number = 0
        require_binary(self.target.native_probe)
        inspect_workerd(self.target, receipt)
        for name, digest in [
            ("index.js", self.target.bundle_js_sha256),
            ("index_bg.wasm", self.target.bundle_wasm_sha256),
        ]:
            if file_digest(self.target.bundle / name) != digest:
                raise ValueError("Worker bundle changed from the declared hashes")
        self.persistence = receipt.directory / "workerd-state"
        self.persistence.mkdir(mode=0o700)
        self.fixture_root = receipt.directory / "jwks"
        self.fixture_root.mkdir(mode=0o700)
        log = (receipt.directory / "jwks.stderr").open("xb")
        self.logs.append(log)
        self.broker: subprocess.Popen[bytes] = subprocess.Popen(
            [str(self.target.native_probe.path), "fixture", str(self.fixture_root)],
            stdin=subprocess.PIPE,
            stdout=subprocess.PIPE,
            stderr=log,
        )
        try:
            ready = child_reply(self.broker)
            if (
                ready.get("kind") != "fixtureReady"
                or ready.get("pid") != self.broker.pid
            ):
                raise ValueError("JWKS fixture identifies another child")
            self.jwks = string_value(ready.get("url"))
            with socket.socket() as port:
                port.bind(("127.0.0.1", 0))
                # AF_INET was bound above; socket types expose getsockname as Any.
                address = t.cast(tuple[str, int], port.getsockname())
                self.port: int = address[1]
            if self.port in {8787, 8788, 43127}:
                raise ValueError("OS allocated a protected port; choose a fresh run")
            self.origin = f"http://127.0.0.1:{self.port}"
            self.owner = HttpEndpoint(
                self.origin,
                {
                    "Cf-Access-Jwt-Assertion": (
                        self.fixture_root / "owner.jwt"
                    ).read_text()
                },
            )
            self.start()
        except Exception:
            self.close()
            raise

    def start(self) -> None:
        self.number += 1
        log = (self.receipt.directory / f"workerd-{self.number}.stderr").open("xb")
        self.logs.append(log)
        self.child = subprocess.Popen(
            [
                str(self.target.node),
                str(
                    self.plan.repository
                    / "scripts/verification/central_staging_workerd.mjs"
                ),
                str(self.target.miniflare),
                str(self.target.bundle),
                str(self.persistence),
                str(self.port),
                self.jwks,
            ],
            cwd=self.plan.repository,
            stdout=subprocess.PIPE,
            stderr=log,
        )
        ready = child_reply(self.child)
        if ready != {
            "kind": "workerdReady",
            "origin": self.origin,
            "pid": self.child.pid,
        }:
            raise ValueError("workerd readiness differs from its exact owned process")
        self.receipt.record(
            {
                "kind": "workerdReady",
                "pid": self.child.pid,
                "lifetime": self.number,
                "persistence": str(self.persistence),
            }
        )

    def restart(self) -> None:
        if self.child is None:
            raise ValueError("no workerd child owned")
        previous = self.child.pid
        exit_code = stop_child(self.child)
        self.child = None
        self.receipt.equal(
            "owned workerd reaped under the requested termination",
            "OB7 local recovery",
            (True, exit_code in {0, -signal.SIGTERM, 128 + signal.SIGTERM}),
        )
        self.receipt.record(
            {"kind": "workerdStopped", "pid": previous, "exit": exit_code}
        )
        self.start()
        self.receipt.record(
            {
                "kind": "workerdRestart",
                "oldPid": previous,
                "newPid": self.child.pid if self.child else None,
                "samePersistence": True,
                "hostedDeploy": False,
            }
        )

    def close(self) -> None:
        if self.child is not None:
            _ = stop_child(self.child)
            self.child = None
        if self.broker.poll() is None and self.broker.stdin is not None:
            self.broker.stdin.close()
            self.broker.stdin = None
            try:
                _ = self.broker.wait(15)
            except subprocess.TimeoutExpired:
                _ = stop_child(self.broker)
        if self.broker.stdout is not None:
            self.broker.stdout.close()
        for log in self.logs:
            log.close()


def session_state(runtime: BoardRuntime) -> SessionState:
    deadline = time.monotonic() + 30
    path = runtime.directory / "machine-session/state.json"
    while time.monotonic() < deadline:
        if path.exists():
            return SessionState.model_validate_json(path.read_bytes())
        time.sleep(0.2)
    raise ValueError("actual Router did not persist its machine session state")


@dataclass(frozen=True, kw_only=True)
class RouterInputs:
    plan: Plan
    receipt: JourneyReceipt
    owner: HttpEndpoint
    origin: str


@t.final
class Routers:
    def __init__(self, inputs: RouterInputs) -> None:
        self.plan, self.receipt, self.owner, self.origin = (
            inputs.plan,
            inputs.receipt,
            inputs.owner,
            inputs.origin,
        )
        self.runtimes: list[BoardRuntime] = []
        self.edges: list[FixtureAccessEdge] = []
        self.credentials: list[str] = []
        self.identities: list[InstallationIdentity] = []
        try:
            self.start()
        except Exception:
            self.close()
            raise

    def start(self) -> None:
        plan, receipt, owner, origin = self.plan, self.receipt, self.owner, self.origin
        target = JourneyTarget(
            plan.repository, plan.expected_head, None, plan.artifact_root
        )
        receipt.target = target.inspect(plan.repository)
        for index in range(2):
            runtime = BoardRuntime(target, receipt, plan.runtime_parent)
            self.runtimes.append(runtime)
            if isinstance(plan.target, StagingTarget):
                enrolled = plan.target.installations[index]
                seed = enrolled.seed_root
                if (
                    seed.resolve(strict=True) != seed
                    or not seed.is_relative_to(plan.runtime_parent)
                    or (seed / "service.json").exists()
                ):
                    raise ValueError(
                        "staging seed must be a stopped dedicated fixture below the private runtime parent"
                    )
                for path in [seed, *seed.rglob("*")]:
                    if path.is_symlink() or path.stat().st_uid != os.getuid():
                        raise ValueError(
                            "staging seed contains a link or different owner"
                        )
                _ = shutil.copytree(seed, runtime.directory, dirs_exist_ok=True)
            runtime.start()
            identity = InstallationIdentity.model_validate(
                BoardClient(runtime).read("installation_show", {})
            )
            runtime.stop()
            self.identities.append(identity)
            if isinstance(plan.target, LocalTarget):
                credential = "g4-fixture-" + fixture_identity()
                digest = hashlib.sha256(credential.encode()).hexdigest()
                enrolled_reply = owner.post(
                    "/admin/v1/machines/enroll",
                    {
                        "kind": "enrollMachine",
                        "value": {
                            "label": identity.machineLabel,
                            "installation": identity.serviceId,
                            "peer": identity.peerId,
                            "access_client_id": f"test-client-{digest}.access",
                            "credential_hash": digest,
                            "expires_at": "2033-05-18T03:33:20Z",
                        },
                    },
                )
                if enrolled_reply.get("kind") != "machineEnrolled":
                    raise ValueError("local synthetic enrollment refused")
                edge = FixtureAccessEdge(plan.repository, origin, credential)
                self.edges.append(edge)
                endpoint, access_id, access_secret = (
                    edge.endpoint,
                    "board-fixture-client",
                    "board-fixture-secret",
                )
                receiver = string_value(
                    field(enrolled_reply, "value", "authoritativeReceiver")
                )
                relay = string_value(field(enrolled_reply, "value", "relayPeer"))
            else:
                enrolled = plan.target.installations[index]
                if (
                    str(enrolled.installation) != identity.serviceId
                    or str(enrolled.peer) != identity.peerId
                ):
                    raise ValueError(
                        "pre-enrolled fixture identity differs from its actual Router identity"
                    )
                credential = normalize_machine_credential(
                    environment_value(enrolled.credentials.machine)
                )
                endpoint = origin + "/v1/session"
                access_id = environment_value(enrolled.credentials.access_id)
                access_secret = environment_value(enrolled.credentials.access_secret)
                receiver, relay = str(plan.target.receiver), str(plan.target.relay)
            self.credentials.append(credential)
            session = runtime.directory / "session.json"
            private_json(
                session,
                {
                    "endpoint": endpoint,
                    "access": {"clientId": access_id, "clientSecret": access_secret},
                    "credential": "Bearer " + credential,
                    "peer": identity.peerId,
                    "receiver": receiver,
                    "relay": relay,
                },
            )
            runtime.fixture_session = session
            runtime.start()
            _ = session_state(runtime)

    def close(self) -> None:
        errors: list[str] = []
        for runtime in reversed(self.runtimes):
            try:
                runtime.stop()
            except Exception as error:  # noqa: BLE001 -- reap every exact child even when one fails
                errors.append(type(error).__name__)
                if runtime.child is not None:
                    _ = stop_child(runtime.child)
                    runtime.child = None
                if runtime.stderr is not None:
                    runtime.stderr.close()
                    runtime.stderr = None
        for edge in reversed(self.edges):
            try:
                edge.close()
            except Exception as error:  # noqa: BLE001 -- same independent ownership
                errors.append(type(error).__name__)
        if errors:
            raise ValueError(
                "owned Router/edge cleanup failed; inspect private receipts"
            )
