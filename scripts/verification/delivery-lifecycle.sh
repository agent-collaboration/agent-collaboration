#!/usr/bin/env bash
# Exact seven native delivery lifecycle identities; missing cases never pass.
set -euo pipefail
verification_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
exec node "$verification_root/verification/tooling/run-fleet-delivery-lifecycle.ts" "$@"
