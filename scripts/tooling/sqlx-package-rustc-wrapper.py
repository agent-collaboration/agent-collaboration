#!/usr/bin/env python3
"""Describe one Cargo package against its schema while dependencies use offline catalogs."""

import os
import sys


def compiler_environment(environment: dict[str, str]) -> dict[str, str]:
    scoped = environment | {"SQLX_OFFLINE": "true"}
    _ = scoped.pop("DATABASE_URL", None)
    _ = scoped.pop("SQLX_OFFLINE_DIR", None)
    if (
        environment.get("CARGO_PKG_NAME") == environment["SQLX_PREPARE_PACKAGE"]
        and environment.get("CARGO_MANIFEST_DIR")
        == environment["SQLX_PREPARE_MANIFEST_DIR"]
    ):
        scoped |= {
            "SQLX_OFFLINE": "false",
            "DATABASE_URL": environment["SQLX_PREPARE_DATABASE_URL"],
            "SQLX_OFFLINE_DIR": environment["SQLX_PREPARE_METADATA_DIR"],
        }
    return scoped


def main() -> int:
    arguments = sys.argv[1:]
    if not arguments:
        print(
            "usage: sqlx-package-rustc-wrapper.py <rustc> [arguments...]",
            file=sys.stderr,
        )
        return 2
    os.execvpe(arguments[0], arguments, compiler_environment(dict(os.environ)))


if __name__ == "__main__":
    raise SystemExit(main())
