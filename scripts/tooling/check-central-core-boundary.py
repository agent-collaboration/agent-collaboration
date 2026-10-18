"""Enforce the platform-free, synchronous central core boundary.

Superseded (2026-10-16): the "no async" rule enforced a boundary the owner never asked for, and a
source-text scan is the kind of pedantic test the owner ruled out (requirements U69, U74). The
one-core rebuild removes this script; see docs/specs/2026-10-16-central-one-core/.
"""

import re
import subprocess
from pathlib import Path


def forbidden_core_construct(line: str) -> bool:
    return re.search(r"\basync\b|cfg\s*\([^\n]*target_arch|(?<!')\bstatic\s+(?:mut\s+)?\w+", line) is not None


def main() -> None:
    result = subprocess.run(
        ["cargo", "tree", "-p", "collaboration-central-core", "-e", "normal", "--prefix", "none"],
        check=False,
        capture_output=True,
        text=True,
    )
    if result.returncode:
        raise SystemExit(result.stderr)
    banned = re.compile(r"^(worker(?:-.*)?|wasm-bindgen(?:-.*)?|js-sys|web-sys|tokio(?:-.*)?|axum(?:-.*)?|reqwest(?:-.*)?|sqlx(?:-.*)?|turso(?:[-_].*)?|rusqlite)$")
    packages = {line.split()[0] for line in result.stdout.splitlines() if line.strip()}
    # The pinned AST parser is pure logic; its macro runs only at compile time.
    # PD3 §2a names these exact exceptions; native Turso remains forbidden.
    parser_packages = {"turso_parser", "turso_macros"}
    violations = sorted(
        package
        for package in packages
        if banned.fullmatch(package) and package not in parser_packages
    )
    source = Path("crates/collaboration-central-core/src")
    for path in source.rglob("*.rs"):
        for number, line in enumerate(path.read_text().splitlines(), start=1):
            if forbidden_core_construct(line):
                violations.append(f"{path}:{number}: forbidden core construct")
    if violations:
        raise SystemExit("\n".join(violations))
    print(f"Central core boundary passed: {len(packages)} normal packages; no platform crates, async, target_arch or global state.")


if __name__ == "__main__":
    main()
