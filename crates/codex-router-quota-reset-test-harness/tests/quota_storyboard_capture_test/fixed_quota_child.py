"""Launch only the declared fixture on the inherited real PTY; preserve teardown."""

import hashlib
import json
import os
import pathlib
import signal
import subprocess
import typing as t
import uuid
from enum import Enum

COMPONENT_TEST = "presentation::quota::quota_browse_presentation_test::quota_storyboard_fixture_test::compiled_quota_component_storyboard_fixture"
COMPONENT_SCENARIOS = {
    "normal": "Normal",
    "inspection-pending": "InspectionPending",
    "commit-pending": "CommitPending",
    "unknown-result": "UnknownResult",
    "save-failed": "SaveFailed",
    "disabled-yes": "DisabledYes",
    "revalidation-pending": "RevalidationPending",
    "refused-result": "RefusedResult",
    "refresh-pending": "RefreshPending",
    "save-pending": "SavePending",
}
PENDING_SCENARIOS = frozenset(
    (
        "inspection-pending",
        "commit-pending",
        "revalidation-pending",
        "refresh-pending",
        "save-pending",
    )
)


class CapturedTuiSurface(Enum):
    QUOTA = "quota"
    SESSIONS = "sessions"
    QUOTA_COMPONENT = "quota-component"
    PUBLIC_SESSIONS = "public-sessions"


def required_text(configuration: dict[str, object], key: str) -> str:
    value = configuration.get(key)
    if not isinstance(value, str) or not value:
        raise ValueError(f"missing fixture text: {key}")
    return value


def owned_future_path(configuration: dict[str, object], key: str) -> pathlib.Path:
    output = pathlib.Path(required_text(configuration, "output"))
    path = pathlib.Path(required_text(configuration, key))
    if (
        not output.is_absolute()
        or not output.is_dir()
        or output.is_symlink()
        or not path.is_absolute()
        or path.parent.resolve() != output.resolve()
        or path.exists()
        or path.is_symlink()
    ):
        raise ValueError(f"{key} must be absent inside the owned output directory")
    return path


def child_launch_configuration(
    configuration: dict[str, object],
) -> tuple[list[str], dict[str, str]]:
    surface = CapturedTuiSurface(required_text(configuration, "surface"))
    binary = required_text(configuration, "binary")
    if surface is CapturedTuiSurface.PUBLIC_SESSIONS:
        allowed = {
            "surface",
            "binary",
            "binary_sha256",
            "approved_cwd",
            "debug_router_root",
            "expected_service_id",
            "expected_service_epoch",
            "output",
            "ttyd",
            "temporary_directory",
            "dynamic_library_path",
        }
        if set(configuration) != allowed:
            raise ValueError(
                "public Sessions requires exact bound configuration fields"
            )
        executable = pathlib.Path(binary)
        if (
            not executable.is_absolute()
            or executable.resolve(strict=True) != executable
            or not executable.is_file()
            or not os.access(executable, os.X_OK)
            or hashlib.sha256(executable.read_bytes()).hexdigest()
            != required_text(configuration, "binary_sha256")
        ):
            raise ValueError("public Sessions binary binding changed")
        for key in ("approved_cwd", "debug_router_root"):
            directory = pathlib.Path(required_text(configuration, key))
            if (
                not directory.is_absolute()
                or directory.resolve(strict=True) != directory
                or not directory.is_dir()
            ):
                raise ValueError("public Sessions directory binding changed")
        root = pathlib.Path(required_text(configuration, "debug_router_root"))
        manifest_path = root / "agent-communication/service.json"
        if (
            manifest_path.is_symlink()
            or manifest_path.resolve(strict=True).parent != root / "agent-communication"
        ):
            raise ValueError("public Sessions Host manifest location changed")
        raw_manifest = t.cast(object, json.loads(manifest_path.read_text()))
        if not isinstance(raw_manifest, dict):
            raise ValueError("public Sessions Host service/epoch mismatch")
        manifest = t.cast(dict[object, object], raw_manifest)
        for field, expected in (
            ("serviceId", "expected_service_id"),
            ("serviceEpoch", "expected_service_epoch"),
        ):
            value = manifest.get(field)
            if not isinstance(value, str) or str(uuid.UUID(value)) != required_text(
                configuration, expected
            ):
                raise ValueError("public Sessions Host service/epoch mismatch")
        environment = dict(os.environ)
        environment["CODEX_ROUTER_DEBUG_ROUTER_ROOT"] = str(root)
        environment["TERM"] = "xterm-256color"
        environment["LANG"] = "C.UTF-8"
        return [binary], environment
    environment = {
        "HOME": required_text(configuration, "temporary_directory"),
        "TERM": "xterm-256color",
        "LANG": "C.UTF-8",
        "TMPDIR": required_text(configuration, "temporary_directory"),
    }
    if surface is CapturedTuiSurface.QUOTA_COMPONENT:
        scenario = required_text(configuration, "component_fixture_scenario")
        if scenario not in COMPONENT_SCENARIOS:
            raise ValueError("unknown component fixture scenario")
        receipt = owned_future_path(configuration, "component_fixture_receipt")
        environment["QUOTA_COMPONENT_FIXTURE"] = scenario
        environment["QUOTA_COMPONENT_FIXTURE_RECEIPT_FILE"] = str(receipt)
        if scenario in PENDING_SCENARIOS:
            release = owned_future_path(configuration, "component_fixture_release")
            if release == receipt:
                raise ValueError("fixture release and completion paths must differ")
            environment["QUOTA_COMPONENT_FIXTURE_RELEASE_FILE"] = str(release)
        return [
            binary,
            COMPONENT_TEST,
            "--exact",
            "--ignored",
            "--nocapture",
            "--test-threads=1",
        ], environment
    if surface is CapturedTuiSurface.QUOTA:
        return [
            binary,
            "--router-root",
            required_text(configuration, "root"),
            "--fixture-capability",
            required_text(configuration, "capability"),
            "--provider-listener",
            required_text(configuration, "provider"),
        ], environment
    return [
        binary,
        "--sessions-picker",
        "--provider-filter-storyboard",
        "--fixture-capability",
        required_text(configuration, "capability"),
    ], environment


def verify_component_completion(configuration: dict[str, object]) -> None:
    scenario = required_text(configuration, "component_fixture_scenario")
    receipt = pathlib.Path(required_text(configuration, "component_fixture_receipt"))
    if not receipt.is_file() or receipt.is_symlink():
        raise ValueError("component fixture completion receipt missing")
    released = scenario in PENDING_SCENARIOS
    if released:
        release = pathlib.Path(
            required_text(configuration, "component_fixture_release")
        )
        if release.is_symlink() or release.read_bytes() != b"settle\n":
            raise ValueError("component fixture lacks exact owned release")
    expected = (
        f"QUOTA_COMPONENT_FIXTURE_FINISHED scenario={COMPONENT_SCENARIOS[scenario]} "
        f"released={str(released).lower()} external_requests=0 persisted_writes=0\n"
    ).encode()
    if receipt.read_bytes() != expected:
        raise ValueError("component fixture completion receipt mismatch")


def read_configuration() -> dict[str, object]:
    raw = t.cast(
        object,
        json.loads(
            pathlib.Path(os.environ["QUOTA_STORYBOARD_FIXTURE_COMMAND"]).read_text()
        ),
    )
    if not isinstance(raw, dict):
        raise TypeError("fixture configuration must be an object")
    decoded = t.cast(dict[object, object], raw)
    configuration: dict[str, object] = {}
    for key, value in decoded.items():
        if not isinstance(key, str) or not isinstance(value, str):
            raise TypeError("fixture configuration requires named text fields")
        configuration[key] = value
    return configuration


def main() -> int:
    configuration = read_configuration()
    arguments, environment = child_launch_configuration(configuration)
    output = pathlib.Path(required_text(configuration, "output"))
    _ = (output / "quota-wrapper.pid").write_text(str(os.getpid()))
    if configuration["surface"] in (
        CapturedTuiSurface.PUBLIC_SESSIONS.value,
        CapturedTuiSurface.QUOTA_COMPONENT.value,
    ):
        identity = subprocess.check_output(
            ["/bin/ps", "-p", str(os.getpid()), "-o", "uid=,lstart=,command="],
            text=True,
            timeout=3,
        )
        if not identity.strip().startswith(str(os.getuid()) + " "):
            raise ValueError("public wrapper UID observation failed")
        _ = (output / "quota-wrapper.identity").write_text(identity)
    cwd = (
        required_text(configuration, "approved_cwd")
        if configuration["surface"] == CapturedTuiSurface.PUBLIC_SESSIONS.value
        else None
    )
    child = subprocess.Popen(arguments, env=environment, cwd=cwd)
    _ = (output / "quota-child.pid").write_text(str(child.pid))

    def terminate_owned_child(_signal_number: int, _frame: object) -> None:
        if child.poll() is None:
            child.terminate()

    def record_viewport(_signal_number: int = 0, _frame: object = None) -> None:
        viewport = os.get_terminal_size(1)
        _ = (output / "quota-child.viewport").write_text(
            f"{viewport.columns} {viewport.lines}"
        )

    _ = signal.signal(signal.SIGTERM, terminate_owned_child)
    _ = signal.signal(signal.SIGINT, terminate_owned_child)
    _ = signal.signal(signal.SIGWINCH, record_viewport)
    record_viewport()
    exit_code = child.wait()
    record_viewport()
    if configuration["surface"] == CapturedTuiSurface.QUOTA_COMPONENT.value:
        _ = (output / "component-process.exit").write_text(str(exit_code))
        if exit_code == 0:
            try:
                verify_component_completion(configuration)
            except (OSError, ValueError) as error:
                _ = (output / "component-receipt.error").write_text(str(error))
                exit_code = 2
    _ = (output / "quota-child.exit").write_text(str(exit_code))
    if configuration["surface"] == CapturedTuiSurface.QUOTA_COMPONENT.value:
        # Keep the validated completion on VHS CurrentLine after PTY scrollback.
        print(f"\nQUOTA_STORYBOARD_CHILD_EXIT={exit_code}", end="", flush=True)
    else:
        print(f"QUOTA_STORYBOARD_CHILD_EXIT={exit_code}", flush=True)
    return exit_code


if __name__ == "__main__":
    raise SystemExit(main())
