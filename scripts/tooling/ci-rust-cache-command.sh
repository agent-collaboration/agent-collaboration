#!/usr/bin/env bash
set -euo pipefail

# rust-cache hashes the active compiler and every toolchain in this list.
# Only the toolchain selected by CI belongs in that list for key construction.
if [[ "$#" == 4 && "$1" == rustup && "$2" == toolchain && "$3" == list && "$4" == --quiet ]]; then
  printf '%s\n' "${RUST_TOOLCHAIN:?CI must select a Rust toolchain}"
else
  exec "$@"
fi
