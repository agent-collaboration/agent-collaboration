#!/usr/bin/env python3
"""Keep dependency macros offline while SQLx CLI describes one SQLite package."""

import os
import sys


def compiler_environment(environment: dict[str, str]) -> dict[str, str]:
    scoped = environment | {"SQLX_OFFLINE": "true"}
    _ = scoped.pop("DATABASE_URL", None)
    _ = scoped.pop("SQLX_OFFLINE_DIR", None)
    if (
        environment.get("CARGO_PKG_NAME") == environment["SQLITE_PREPARE_PACKAGE"]
        and environment.get("CARGO_MANIFEST_DIR")
        == environment["SQLITE_PREPARE_MANIFEST_DIR"]
    ):
        scoped |= {
            "SQLX_OFFLINE": "false",
            "DATABASE_URL": environment["SQLITE_PREPARE_DATABASE_URL"],
        }
        # SQLx CLI chooses a private output directory for its exact comparison.
        if "SQLX_OFFLINE_DIR" in environment:
            scoped["SQLX_OFFLINE_DIR"] = environment["SQLX_OFFLINE_DIR"]
    return scoped


def main() -> int:
    arguments = sys.argv[1:]
    if not arguments:
        print(
            "usage: sqlx-sqlite-rustc-wrapper.py <rustc> [arguments...]",
            file=sys.stderr,
        )
        return 2
    os.execvpe(arguments[0], arguments, compiler_environment(dict(os.environ)))


if __name__ == "__main__":
    raise SystemExit(main())
