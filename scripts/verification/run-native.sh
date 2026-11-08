#!/usr/bin/env bash
# Run one relocated consumer-fit journey against real native fleet owners.
# Exit 0 complete; 1 contract mismatch; 2 setup failure; 3 missing coverage.
set -euo pipefail
verification_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
journey_name="${1:-}"
if [[ $# -gt 0 ]]; then shift; fi
case "$journey_name" in
  holder-refusals|target-capability|frame-shapes|copy-containment) ;;
  *) printf 'usage: scripts/verification/<journey>.sh [--help] [--no-build] [--evidence-dir DIR] [--target native|workerd]\n' >&2; exit 2 ;;
esac
build_requested=true
runtime_target=native
evidence_directory=""
while [[ $# -gt 0 ]]; do
  case "$1" in
    --help|-h)
      printf '%s\n' "$journey_name: real native consumer-fit observation."
      printf '%s\n' 'Run from any directory. Coordinate exclusive Cargo access first.'
      printf '%s\n' 'Options: --no-build, --evidence-dir DIR (must not exist), --target native|workerd.'
      printf '%s\n' 'Default evidence: tmp/fleet-verification/<journey>-<unique>.'
      printf '%s\n' 'Uses normal target/debug, test-support/test-fault and the debug signing runner.'
      printf '%s\n' 'Exit 0 complete; 1 mismatch; 2 setup failure; 3 coverage gap.'
      printf '%s\n' 'Copy containment retains two separate-proof gaps even when native controls pass.'
      exit 0 ;;
    --no-build) build_requested=false; shift ;;
    --evidence-dir|--target)
      if [[ $# -lt 2 || -z "$2" ]]; then printf 'missing value for %s\n' "$1" >&2; exit 2; fi
      if [[ "$1" == --evidence-dir ]]; then evidence_directory="$2"; else runtime_target="$2"; fi
      shift 2 ;;
    *) printf 'unknown argument: %s\n' "$1" >&2; exit 2 ;;
  esac
done
cd "$verification_root"
case "$runtime_target" in native|workerd) ;; *) printf 'unsupported target: %s\n' "$runtime_target" >&2; exit 2 ;; esac
if [[ "${CARGO_TARGET_DIR:-$verification_root/target}" != "$verification_root/target" ]]; then
  printf '%s\n' 'CARGO_TARGET_DIR must be the assigned normal worktree target.' >&2
  exit 2
fi
if [[ -n "$evidence_directory" ]]; then
  mkdir "$evidence_directory" || exit 2
else
  mkdir -p tmp/fleet-verification
  evidence_directory="$(mktemp -d "tmp/fleet-verification/${journey_name}-XXXXXX")"
fi
evidence_directory="$(cd "$evidence_directory" && pwd)"
printf 'evidence: %s\n' "$evidence_directory"
record_source() {
  local suffix="$1"
  git rev-parse HEAD > "$evidence_directory/source-head${suffix}.txt"
  git status --porcelain > "$evidence_directory/source-status${suffix}.txt"
  find crates scripts/verification -type f \( -name '*.rs' -o -name '*.sql' -o -name '*.sh' -o -name Cargo.toml -o -name Cargo.lock \) ! -path '*/target/*' -print0 |
    LC_ALL=C sort -z | xargs -0 shasum -a 256 > "$evidence_directory/source-fingerprint${suffix}.txt"
  shasum -a 256 Cargo.toml Cargo.lock .cargo/config.toml scripts/cargo_debug_signing_runner.sh >> "$evidence_directory/source-fingerprint${suffix}.txt"
}
if [[ "$(git rev-parse --show-toplevel)" != "$verification_root" ]]; then
  printf '%s\n' 'Run against the declared repository worktree.' >&2
  exit 2
fi
record_source ''
if [[ "$runtime_target" == workerd ]]; then
  printf '%s\n' '{"status":"gap","case":"workerd-host","detail":"No real fleet session host is exposed by the workerd copy fixture; adapter/SQL proof does not observe session containment."}' | tee "$evidence_directory/observations.jsonl"
  printf '3\n' > "$evidence_directory/exit-code.txt"
  exit 3
fi
export CARGO_TARGET_DIR="$verification_root/target"
consumer_binary="$CARGO_TARGET_DIR/debug/fleet-sync-test-consumer"
delivery_binary="$CARGO_TARGET_DIR/debug/delivery_node"
journey_binary="$CARGO_TARGET_DIR/debug/examples/fleet-consumer-fit"
if "$build_requested"; then
  df -h . > "$evidence_directory/disk-before.txt"
  if [[ "$(df -k . | awk 'NR==2 {print $4}')" -lt 31457280 ]]; then
    printf '%s\n' 'Build refused: less than 30 GiB free.' >&2
    exit 2
  fi
  if ! env -u GITHUB_ACTIONS cargo build --locked -p fleet-native-services --features test-support,test-fault --bins > "$evidence_directory/build-consumer.log" 2>&1; then
    printf 'consumer build failed; see %s/build-consumer.log\n' "$evidence_directory" >&2
    exit 2
  fi
  if ! env -u GITHUB_ACTIONS cargo build --locked -p fleet-native-services --features test-support,test-fault --example fleet-consumer-fit > "$evidence_directory/build-driver.log" 2>&1; then
    printf 'journey build failed; see %s/build-driver.log\n' "$evidence_directory" >&2
    exit 2
  fi
fi
if [[ ! -x "$consumer_binary" || ! -x "$delivery_binary" || ! -x "$journey_binary" ]]; then
  printf '%s\n' 'Missing built consumer, delivery fixture or journey executable; see --help.' >&2
  exit 2
fi
runner="$verification_root/scripts/cargo_debug_signing_runner.sh"
if ! env -u GITHUB_ACTIONS bash "$runner" --sign-only "$consumer_binary" "$delivery_binary" "$journey_binary" > "$evidence_directory/signing.log" 2>&1; then
  printf 'signing failed; see %s/signing.log\n' "$evidence_directory" >&2
  exit 2
fi
if [[ "$(uname -s)" == Darwin ]]; then
  for executable in "$consumer_binary" "$delivery_binary" "$journey_binary"; do
    if ! codesign --verify --strict "$executable" 2>> "$evidence_directory/signing.log"; then
      printf 'signature verification failed: %s\n' "$executable" >&2
      exit 2
    fi
    signature_details="$(codesign -dv --verbose=4 "$executable" 2>&1)"
    printf '%s\n%s\n' "$executable" "$signature_details" >> "$evidence_directory/signing.log"
    if ! grep -Fxq 'TeamIdentifier=974QD84WVC' <<< "$signature_details"; then
      printf 'unexpected signing team: %s\n' "$executable" >&2
      exit 2
    fi
  done
fi
shasum -a 256 "$consumer_binary" "$delivery_binary" "$journey_binary" > "$evidence_directory/binary-sha256.txt"
set +e
env -u GITHUB_ACTIONS "CARGO_BIN_EXE_fleet-sync-test-consumer=$consumer_binary" "CARGO_BIN_EXE_delivery_node=$delivery_binary" bash "$runner" "$journey_binary" "$journey_name" "$evidence_directory" 2>&1 | tee "$evidence_directory/observations.jsonl"
journey_exit=${PIPESTATUS[0]}
set -e
printf '%s\n' "$journey_exit" > "$evidence_directory/exit-code.txt"
record_source '-after'
if ! cmp -s "$evidence_directory/source-head.txt" "$evidence_directory/source-head-after.txt" || ! cmp -s "$evidence_directory/source-status.txt" "$evidence_directory/source-status-after.txt" || ! cmp -s "$evidence_directory/source-fingerprint.txt" "$evidence_directory/source-fingerprint-after.txt"; then
  printf '%s\n' '{"status":"gap","case":"source-changed","detail":"Source changed during observation; declare a stable target before reuse."}' | tee -a "$evidence_directory/observations.jsonl"
  exit 3
fi
exit "$journey_exit"
