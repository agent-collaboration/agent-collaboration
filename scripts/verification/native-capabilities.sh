#!/bin/sh
# Native-capabilities journeys J1-J4: runs the Rust verifier example.
# Journey: .agents/skills/verify-codex-router/references/native-capabilities.md
set -eu
repository_root=$(CDPATH= cd -- "$(dirname -- "$0")/../.." && pwd)
cd "$repository_root"
exec cargo run --quiet --locked -p codex-router-host --example verify-provider-native-capabilities -- "$@"
