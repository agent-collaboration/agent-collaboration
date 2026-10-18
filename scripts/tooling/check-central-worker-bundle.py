#!/usr/bin/env python3
"""Check the real Worker artifact for test routes and the G18 panic marker."""

import argparse
import re
import typing as t
from pathlib import Path


BUILD_DIRECTORY: t.Final[Path] = Path("crates/collaboration-central-cloudflare/build")
FAULT_MARKERS: t.Final[tuple[bytes, ...]] = (
    b"G18 conformance-only transaction panic",
    b"/__conformance/control",
    b"_cf_test_directory_updates",
    b"_cf_test_flag_",
    b"_cf_test_known",
    b"_cf_test_clock",
    b"_cf_test_held_dispatch",
    b"edge-probe-unknown",
    b"conformanceClose",
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
    text_values = re.findall(r'type:\s*"text",\s*value:\s*"([^"]*)"', worker_config)
    if not text_values or any(
        not value.startswith("REPLACE_") for value in text_values
    ):
        return "Worker public template vars must be placeholders"
    if "noBundle: true" not in tooling_config:
        return "Worker deploy tooling must upload the guarded bundle without rebundling"
    return None


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("mode", choices=("production", "faults"))
    args = parser.parse_args()
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
    if args.mode == "production" and b"_cf_test_directory_updates" in content:
        print("Worker production bundle contains the test-only Fleet receiver")
        return 1
    if b"central depth measurement" in content:
        print("Worker bundle contains the temporary depth instrument")
        return 1
    present = tuple(marker in content for marker in FAULT_MARKERS)
    expected = (args.mode == "faults",) * len(FAULT_MARKERS)
    if present != expected:
        print(
            f"Worker {args.mode} bundle marker mismatch: {present}; expected {expected}"
        )
        return 1
    print(f"Worker {args.mode} bundle: fault-control marker presence {present}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
