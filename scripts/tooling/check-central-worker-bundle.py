#!/usr/bin/env python3
"""Keep current conformance controls out of release artifacts and retired paths out of both."""

import argparse
import re
import tomllib
import typing as t
from pathlib import Path

BUILD_DIRECTORY: t.Final[Path] = Path("crates/collaboration-central-cloudflare/build")
PRODUCTION_BUILD: t.Final[str] = (
    "worker-build crates/collaboration-central-cloudflare --profile worker-release "
    "--no-opt -- --locked --no-default-features"
)
CONFORMANCE_MARKERS: t.Final[tuple[bytes, ...]] = (
    b"/admin/v1/conformance/bootstrap",
    b"/admin/v1/conformance/runtime",
    b"dropNextObjectReply",
    b"awaitDroppedObjectReply",
    b"resumeDispatchAndHoldNext",
    b"/admin/v1/conformance/fleet-receipt",
    b"readClaimReservation",
    b"/admin/v1/conformance/revocation",
    b"/admin/v1/conformance/access",
    b"/admin/v1/conformance/holder-call",
    b"/admin/v1/conformance/holder-status",
    b"/admin/v1/conformance/registry-dispatches",
    b"/admin/v1/conformance/copy-call",
    b"/admin/v1/conformance/copy-status",
    b"/admin/v1/conformance/copy-wait",
    b"/admin/v1/conformance/drive-tokens",
    # The public admission URI is not a retained byte literal in the qualified artifact.
    # These actual forwarding targets pin that same conformance handler.
    b"/test/admission-origins",
    b"/test/schema-report",
    b"/test/holder-frames",
    b"/test/session-connections",
    b"/test/session-attachment-codec",
    b"/test/session-alarm",
    b"/test/session-faults",
    b"/test/session-finalize-barrier",
    b"/test/session-finalize-journal",
    b"/test/holder-session-identity",
    b"/test/holder-observation",
    b"/test/runtime-control",
    b"_cf_composed_clock",
    b"BootstrapObject",
)
RETIRED_MARKERS: t.Final[tuple[bytes, ...]] = (
    b"G18 conformance-only transaction panic",
    b"/__conformance/control",
    b"/__conformance/core-session",
    b"_cf_test_directory_updates",
    b"_cf_test_flag_",
    b"_cf_test_known",
    b"_cf_test_clock",
    b"_cf_test_held_dispatch",
    b"edge-probe-unknown",
    b"conformanceClose",
    b"LegacyConnectionObject",
    b"LegacyProjectCopyObject",
    b"LegacyLeaseRegistryObject",
    b"LegacyFleetObject",
)


def check_deploy_template(worker_config: str, tooling_config: str) -> str | None:
    """Check the public cf-format templates; returns the failure, if any.

    workers.dev and preview URLs stay off, the account and custom domain stay
    out of the public repository, every text binding is a placeholder, and the
    build step uploads the guarded bundle without rebundling it.
    """
    for setting in ("workersDev: false", "previewUrls: false"):
        if setting not in worker_config:
            return f"Worker deploy template must set {setting}"
    if re.search(r"\b(accountId|domains|routes)\s*:", worker_config):
        return "Worker public template must omit private account and routing values"
    text_values: list[str] = re.findall(
        r'type:\s*"text",\s*value:\s*"([^"]*)"', worker_config
    )
    if not text_values or any(
        not value.startswith("REPLACE_") for value in text_values
    ):
        return "Worker public template vars must be placeholders"
    if "noBundle: true" not in tooling_config:
        return "Worker deploy tooling must upload the guarded bundle without rebundling"
    return None


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    _ = parser.add_argument("mode", choices=("production", "faults"))
    args = parser.parse_args()
    # argparse validates this finite choice before returning its untyped Namespace.
    mode = t.cast(t.Literal["production", "faults"], args.mode)
    # TOML supplies the table; the exact comparison below validates its settings.
    profiles = t.cast(
        dict[str, object],
        tomllib.loads(Path("Cargo.toml").read_text()).get("profile", {}),
    )
    if profiles.get("worker-release") != {
        "inherits": "release",
        "lto": True,
        "opt-level": "s",
        "codegen-units": 1,
    }:
        print("Worker release profile must preserve the standalone size settings")
        return 1
    if (
        PRODUCTION_BUILD
        not in Path("crates/collaboration-central-cloudflare/README.md").read_text()
    ):
        print("Worker production prebuild must select --profile worker-release")
        return 1
    artifacts = [BUILD_DIRECTORY / "index.js", BUILD_DIRECTORY / "index_bg.wasm"]
    if any(not artifact.is_file() for artifact in artifacts):
        print("Worker bundle missing; build it before checking")
        return 1
    content = b"\n".join(artifact.read_bytes() for artifact in artifacts)
    for forbidden in [b"ADMIN_CREDENTIAL_HASH", b"ACCESS_TEST_JWKS_URL"]:
        if forbidden in content:
            print("Worker bundle contains a retired credential or key-source override")
            return 1
    failure = check_deploy_template(
        Path(
            "crates/collaboration-central-cloudflare/cloudflare.config.ts"
        ).read_text(),
        Path("crates/collaboration-central-cloudflare/wrangler.config.ts").read_text(),
    )
    if failure is not None:
        print(failure)
        return 1
    if mode == "production" and b"_cf_test_directory_updates" in content:
        print("Worker production bundle contains the test-only Fleet receiver")
        return 1
    if b"central depth measurement" in content:
        print("Worker bundle contains the temporary depth instrument")
        return 1
    if any(marker in content for marker in RETIRED_MARKERS):
        print("Worker bundle contains a retired central test path")
        return 1
    present = tuple(marker in content for marker in CONFORMANCE_MARKERS)
    expected = (mode == "faults",) * len(CONFORMANCE_MARKERS)
    if present != expected:
        print(f"Worker {mode} bundle marker mismatch: {present}; expected {expected}")
        return 1
    print(f"Worker {mode} bundle: conformance-control marker presence {present}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
