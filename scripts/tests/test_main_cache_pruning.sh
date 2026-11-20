#!/usr/bin/env bash
# Exercise deletion selection and the real CLI wrapper without deleting caches.
set -euo pipefail
repository_root="$(git rev-parse --show-toplevel)"
prune_script="$repository_root/scripts/tooling/prune-main-caches.sh"
cache_command="$repository_root/scripts/tooling/ci-rust-cache-command.sh"
fixture_root="$(mktemp -d)"
trap 'rm -rf "$fixture_root"' EXIT
writers_started_at=2026-10-27T01:00:00Z

# The command wrapper supplies the pinned toolchain and forwards actual probes.
[[ "$(RUST_TOOLCHAIN=1.98.1 bash "$cache_command" rustup toolchain list --quiet)" == 1.98.1 ]]
[[ "$(bash "$cache_command" printf '%s\n' 'argument with spaces')" == 'argument with spaces' ]]
cat > "$fixture_root/compiler-probe" <<'PROBE'
#!/usr/bin/env bash
set -euo pipefail
case "${0##*/}:$#:$*" in
  'rustc:1:-vV') printf '%s\n' 'active compiler' ;;
  'rustup:4:run 1.98.1 rustc -vV') printf '%s\n' 'selected compiler' ;;
  'cargo:5:metadata --all-features --format-version 1 --no-deps') printf '%s\n' '{"workspace_members":[]}' ;;
  'cargo:4:metadata --all-features --format-version 1') exit 23 ;;
  *) exit 99 ;;
esac
PROBE
chmod +x "$fixture_root/compiler-probe"
for probe in rustc rustup cargo; do
  ln -s compiler-probe "$fixture_root/$probe"
done
[[ "$(PATH="$fixture_root:$PATH" bash "$cache_command" rustc -vV)" == 'active compiler' ]]
[[ "$(PATH="$fixture_root:$PATH" bash "$cache_command" rustup run 1.98.1 rustc -vV)" == 'selected compiler' ]]
[[ "$(PATH="$fixture_root:$PATH" bash "$cache_command" cargo metadata --all-features --format-version 1 --no-deps)" == '{"workspace_members":[]}' ]]
metadata_exit=0
PATH="$fixture_root:$PATH" bash "$cache_command" cargo metadata --all-features --format-version 1 || metadata_exit=$?
[[ "$metadata_exit" == 23 ]]

# A stable replacement makes the retired namespace eligible for cleanup.
printf '%s\n' '[
  {"id":1,"key":"v0-rust-e2e-Linux-x64-aaaaaaaa-11111111","ref":"refs/heads/main","createdAt":"2026-10-26T01:00:00Z"},
  {"id":2,"key":"v1-rust-e2e-Linux-x64-bbbbbbbb-22222222","ref":"refs/heads/main","createdAt":"2026-10-27T01:00:01Z"}
]' > "$fixture_root/migration.json"
bash "$prune_script" --plan "$writers_started_at" < "$fixture_root/migration.json" |
  jq -e 'length == 1 and .[0].keepKey == "v1-rust-e2e-Linux-x64-bbbbbbbb-22222222" and [.[].delete[].id] == [1]' > /dev/null
# A prior successful save can finish an interrupted legacy cleanup on an exact hit.
bash "$prune_script" --plan 2026-10-28T01:00:00Z < "$fixture_root/migration.json" |
  jq -e 'length == 1 and .[0].reason == "Stable CI replacement already present" and [.[].delete[].id] == [1]' > /dev/null
# A failed or missing save leaves the old cache available for rollback.
jq '[.[] | select(.id == 1)]' "$fixture_root/migration.json" |
  bash "$prune_script" --plan "$writers_started_at" |
  jq -e 'length == 1 and .[0].reason == "No stable CI replacement available" and .[0].delete == []' > /dev/null

# Distinct prior stable keys give no basis for choosing one after a cache hit.
jq '. + [.[1] + {id:3,key:"v1-rust-e2e-Linux-x64-cccccccc-33333333"}]' "$fixture_root/migration.json" |
  bash "$prune_script" --plan 2026-10-28T01:00:00Z |
  jq -e 'length == 1 and .[0].reason == "No key saved in this run" and .[0].delete == []' > /dev/null

jq -n '
  ["v1-rust-static-", "v1-rust-test-", "v1-rust-e2e-", "v1-rust-wasm-",
   "Linux-X64-rust-tools-",
   "Linux-X64-rust-tools-sqlx-", "Linux-X64-worker-build-"]
  | to_entries | map(
      (.value | startswith("v1-rust-")) as $rust_cache
      | [{id: (100 + .key), key: (.value + (if $rust_cache then "Linux-x64-aaaaaaaa-11111111" else "z-old" end)), ref: "refs/heads/main",
        createdAt: "2026-10-26T01:00:00Z", sizeInBytes: 10},
       {id: (200 + .key), key: (.value + (if $rust_cache then "Linux-x64-aaaaaaaa-22222222" else "a-new" end)), ref: "refs/heads/main",
        createdAt: "2026-10-27T01:00:01.2Z", sizeInBytes: 20}]) | add
  | . + [{id: 900, key: "v0-rust-static-pr", ref: "refs/pull/1/merge",
          createdAt: "2026-10-27T01:00:02Z", sizeInBytes: 30},
         {id: 901, key: "v0-rust-static-tag", ref: "refs/tags/v1.0.0",
          createdAt: "2026-10-26T01:00:00Z", sizeInBytes: 40},
         {id: 902, key: "unrelated-cache", ref: "refs/heads/main",
          createdAt: "2026-10-26T01:00:00Z", sizeInBytes: 50}]
  | reverse
' > "$fixture_root/inventory.json"

bash "$prune_script" --plan "$writers_started_at" < "$fixture_root/inventory.json" > "$fixture_root/plan.json"
jq -e 'length == 7 and ([.[].delete[].id] | sort) == [100,101,102,103,104,105,106]
  and all(.[]; .keepKey | endswith("-22222222") or endswith("a-new"))' "$fixture_root/plan.json" > /dev/null

# An exact hit has only one key and creates no new archive.
jq 'map(select(.id >= 200) | .createdAt = "2026-10-26T01:00:00Z")' "$fixture_root/inventory.json" > "$fixture_root/hits.json"
bash "$prune_script" --plan "$writers_started_at" < "$fixture_root/hits.json" |
  jq -e 'length == 7 and all(.[]; .reason == "No key saved in this run" and .delete == [])' > /dev/null
# Never delete an older cache version under the protected key either.
jq '. + [(.[] | select(.id == 200)) + {id: 903, createdAt: "2026-10-26T00:00:00Z"}]' "$fixture_root/inventory.json" > "$fixture_root/versions.json"
bash "$prune_script" --plan "$writers_started_at" < "$fixture_root/versions.json" | jq -e '([.[].delete[].id] | sort) == [100,101,102,103,104,105,106]' > /dev/null

# Compare fractional seconds accurately at the writer-start boundary.
printf '%s\n' '[
  {"id":1,"key":"v1-rust-static-Linux-x64-aaaaaaaa-11111111","ref":"refs/heads/main","createdAt":"2026-10-27T01:00:00.09Z"},
  {"id":2,"key":"v1-rust-static-Linux-x64-aaaaaaaa-22222222","ref":"refs/heads/main","createdAt":"2026-10-27T01:00:00.2Z"}
]' | bash "$prune_script" --plan 2026-10-27T01:00:00.100000001Z | jq -e '.[0].keepKey == "v1-rust-static-Linux-x64-aaaaaaaa-22222222" and [.[].delete[].id] == [1]' > /dev/null

# A run with exact hits can have several older keys in the same group.
jq 'map(.createdAt = "2026-10-26T01:00:00Z")' "$fixture_root/inventory.json" > "$fixture_root/no-saves.json"
bash "$prune_script" --plan "$writers_started_at" < "$fixture_root/no-saves.json" > "$fixture_root/no-saves-plan.json"
jq -e 'length == 7 and all(.[]; .keepKey == null and .reason == "No key saved in this run" and .delete == [])' \
  "$fixture_root/no-saves-plan.json" > /dev/null

# Multiple saves, invalid IDs and truncated inventories fail closed.
expect_plan_failure() {
  local plan_exit=0
  bash "$prune_script" --plan "$writers_started_at" < "$1" \
    > "$fixture_root/refused.stdout" 2> "$fixture_root/refused.stderr" || plan_exit=$?
  if [[ "$plan_exit" != 5 || -s "$fixture_root/refused.stdout" || "$(cat "$fixture_root/refused.stderr")" != *"$2"* ]]; then
    echo "Expected exit 5, no plan and diagnostic containing: $2" >&2
    exit 1
  fi
}
jq 'map(.createdAt = "2026-10-27T01:00:01Z")' "$fixture_root/inventory.json" > "$fixture_root/multiple.json"
expect_plan_failure "$fixture_root/multiple.json" 'Multiple keys saved in this run'
jq '.[0].id = "not-an-id"' "$fixture_root/inventory.json" > "$fixture_root/invalid.json"
expect_plan_failure "$fixture_root/invalid.json" 'Invalid or duplicate cache identity'
jq '.[0].id = .[1].id' "$fixture_root/inventory.json" > "$fixture_root/duplicate.json"
expect_plan_failure "$fixture_root/duplicate.json" 'Invalid or duplicate cache identity'
jq -n '[range(1000) | {id: (1 + .), key: "unrelated", ref: "refs/heads/main"}]' > "$fixture_root/truncated.json"
expect_plan_failure "$fixture_root/truncated.json" 'Cache inventory must be a complete array with fewer than 1000 entries'

# A fresh Static save must preserve a restore-only reader's different prefix.
printf '%s\n' '[
  {"id":90,"key":"v1-rust-static-Linux-x64-46306b92-415577a5","ref":"refs/heads/main","createdAt":"2026-11-19T14:33:10Z","sizeInBytes":10},
  {"id":91,"key":"v1-rust-static-Linux-x64-9e6faf69-415577a5","ref":"refs/heads/main","createdAt":"2026-11-19T16:34:00Z","sizeInBytes":10}
]' | bash "$prune_script" --plan 2026-11-19T16:33:00Z |
  jq -e 'length == 1 and .[0].keepKey == "v1-rust-static-Linux-x64-9e6faf69-415577a5" and .[0].delete == []' > /dev/null

# One current save supersedes older keys in its prefix and preserves other prefixes.
jq '. + [.[] | select(.id >= 200 and .id <= 203)
  | .id += 200 | .createdAt = "2026-10-26T01:00:00Z" | .key |= sub("-aaaaaaaa-"; "-bbbbbbbb-")]' \
  "$fixture_root/inventory.json" > "$fixture_root/rust-split.json"
bash "$prune_script" --plan "$writers_started_at" < "$fixture_root/rust-split.json" > "$fixture_root/rust-split-plan.json"
jq -e 'length == 7 and ([.[].delete[].id] | sort) == [100,101,102,103,104,105,106]' \
  "$fixture_root/rust-split-plan.json" > /dev/null

jq 'map(if .id == 403 then .createdAt = "2026-10-27T01:00:02Z" else . end)' \
  "$fixture_root/rust-split.json" > "$fixture_root/rust-ambiguous.json"
expect_plan_failure "$fixture_root/rust-ambiguous.json" 'Multiple keys saved in this run for v1-rust-wasm-Linux-x64'
jq 'map(if .id == 403 then .key = "v1-rust-wasm-unrecognized" else . end)' \
  "$fixture_root/rust-split.json" > "$fixture_root/rust-unknown.json"
expect_plan_failure "$fixture_root/rust-unknown.json" 'Unexpected rust-cache key: v1-rust-wasm-unrecognized'
jq 'map(if .id == 403 then .key = "v0-rust-unknown-Linux-x64-bbbbbbbb-22222222" else . end)' \
  "$fixture_root/rust-split.json" > "$fixture_root/rust-unknown-family.json"
expect_plan_failure "$fixture_root/rust-unknown-family.json" 'Unexpected rust-cache key: v0-rust-unknown-Linux-x64-bbbbbbbb-22222222'

# Release workflow caches: several main keys, none saved by this run, are left alone.
jq '. + [
  {id:400,key:"v0-rust-release-arm64-background-Darwin-arm64-aaaaaaaa-11111111",ref:"refs/heads/main",createdAt:"2026-10-20T01:00:00Z",sizeInBytes:10},
  {id:401,key:"v0-rust-release-arm64-background-Darwin-arm64-aaaaaaaa-22222222",ref:"refs/heads/main",createdAt:"2026-10-27T01:00:01Z",sizeInBytes:20}
]' "$fixture_root/inventory.json" > "$fixture_root/with-release.json"
if ! bash "$prune_script" --plan "$writers_started_at" < "$fixture_root/with-release.json" |
  jq -e '([.[].delete[].id] | sort) == [100,101,102,103,104,105,106] and all(.[]; .family | contains("rust-release-") | not)' > /dev/null; then
  echo 'Release workflow caches must never be selected or block pruning' >&2
  exit 1
fi

# pnpm follows the same save rule and protects other refs.
jq '. + [
  {id:300,key:"node-cache-Linux-x64-pnpm-old-a",ref:"refs/heads/main",createdAt:"2026-10-26T01:00:00Z",sizeInBytes:10},
  {id:301,key:"node-cache-Linux-x64-pnpm-old-b",ref:"refs/heads/main",createdAt:"2026-10-26T02:00:00Z",sizeInBytes:20},
  {id:930,key:"node-cache-Linux-x64-pnpm-pr",ref:"refs/pull/1/merge",createdAt:"2026-10-27T01:00:01Z",sizeInBytes:30},
  {id:931,key:"node-cache-Linux-x64-pnpm-tag",ref:"refs/tags/v1.0.0",createdAt:"2026-10-27T01:00:01Z",sizeInBytes:40},
  {id:932,key:"node-cache-Linux-x64-npm-other",ref:"refs/heads/main",createdAt:"2026-10-26T01:00:00Z",sizeInBytes:50}
]' "$fixture_root/inventory.json" > "$fixture_root/with-pnpm.json"
bash "$prune_script" --plan "$writers_started_at" < "$fixture_root/with-pnpm.json" > "$fixture_root/pnpm-plan.json"
jq -e '([.[].delete[].id] | sort) == [100,101,102,103,104,105,106]
  and any(.[]; .family == "node-cache-Linux-x64-pnpm-" and .reason == "No key saved in this run" and .delete == [])' \
  "$fixture_root/pnpm-plan.json" > /dev/null
jq 'map(if .id == 300 then .createdAt = "2026-10-27T01:00:01Z" else . end)' "$fixture_root/with-pnpm.json" > "$fixture_root/fresh-pnpm.json"
bash "$prune_script" --plan "$writers_started_at" < "$fixture_root/fresh-pnpm.json" |
  jq -e '([.[].delete[].id] | sort) == [100,101,102,103,104,105,106,301]
    and any(.[]; .family == "node-cache-Linux-x64-pnpm-" and .keepKey == "node-cache-Linux-x64-pnpm-old-a")' > /dev/null

# Stub only the remote service; run the actual apply guard, plan and delete loop.
cat > "$fixture_root/gh" <<'GH'
#!/usr/bin/env bash
set -euo pipefail
printf '%s\n' "$*" >> "$GH_CALLS"
if [[ "$1 $2" == 'run view' ]]; then
  cat "$GH_JOB_FIXTURE"
elif [[ "$1 $2" == 'cache list' ]]; then
  cat "$GH_FIXTURE"
elif [[ "$1 $2" == 'cache delete' ]]; then
  [[ "${GH_DELETE_FAILURE:-0}" == 0 ]] || exit 9
else
  exit 10
fi
GH
chmod +x "$fixture_root/gh"
export GH_CALLS="$fixture_root/calls" GH_FIXTURE="$fixture_root/inventory.json"
printf '%s\n' '{"createdAt":"2026-10-26T01:00:00Z","jobs":[
  {"name":"Prune superseded main caches","startedAt":"2026-10-27T02:00:00Z","conclusion":null},
  {"name":"Central Worker build and local workerd conformance (2/2)","startedAt":"2026-10-27T01:00:00Z","conclusion":"success"},
  {"name":"Central Worker build and local workerd conformance (1/2)","startedAt":"2026-10-27T01:00:02Z","conclusion":"success"},
  {"name":"Static validation","startedAt":"2026-10-27T01:00:03Z","conclusion":"success"},
  {"name":"Test Rust workspace (1/2)","startedAt":"2026-10-27T01:00:03Z","conclusion":"failure"},
  {"name":"Test Rust workspace (2/2)","startedAt":"0001-01-01T00:00:00Z","conclusion":"skipped"}
]}' > "$fixture_root/jobs.json"
export GH_JOB_FIXTURE="$fixture_root/jobs.json" GITHUB_RUN_ID=1 GITHUB_RUN_ATTEMPT=1
export PATH="$fixture_root:$PATH"
for event_ref in 'pull_request refs/pull/1/merge' 'push refs/tags/v1.0.0' 'workflow_dispatch refs/heads/main'; do
  : > "$GH_CALLS"
  read -r event_name event_ref_name <<< "$event_ref"
  if GITHUB_EVENT_NAME="$event_name" GITHUB_REF="$event_ref_name" GITHUB_REPOSITORY=example/repo \
    bash "$prune_script" --apply > /dev/null 2>&1; then
    echo "Deletion was allowed for $event_ref" >&2
    exit 1
  fi
  [[ ! -s "$GH_CALLS" ]]
done

: > "$GH_CALLS"
GITHUB_EVENT_NAME=push GITHUB_REF=refs/heads/main GITHUB_REPOSITORY=example/repo \
  bash "$prune_script" --apply > /dev/null
awk '$1 == "cache" && $2 == "delete" {print $3}' "$GH_CALLS" | jq -s -e 'sort == [100,101,102,103,104,105,106]' > /dev/null
[[ "$(wc -l < "$GH_CALLS" | tr -d ' ')" == 10 ]]

: > "$GH_CALLS"
GITHUB_EVENT_NAME=push GITHUB_REF=refs/heads/main GITHUB_REPOSITORY=example/repo \
  GH_FIXTURE="$fixture_root/no-saves.json" bash "$prune_script" --apply > /dev/null
awk '$1 == "cache" && $2 == "delete" {print $3}' "$GH_CALLS" | jq -s -e 'length == 0' > /dev/null
[[ "$(wc -l < "$GH_CALLS" | tr -d ' ')" == 3 ]]

: > "$GH_CALLS"
if GITHUB_EVENT_NAME=push GITHUB_REF=refs/heads/main GITHUB_REPOSITORY=example/repo \
  GH_FIXTURE="$fixture_root/multiple.json" bash "$prune_script" --apply > /dev/null 2>&1; then
  echo 'Ambiguous apply unexpectedly succeeded' >&2
  exit 1
fi
[[ "$(wc -l < "$GH_CALLS" | tr -d ' ')" == 2 ]]

: > "$GH_CALLS"
set +e
GITHUB_EVENT_NAME=push GITHUB_REF=refs/heads/main GITHUB_REPOSITORY=example/repo GH_DELETE_FAILURE=1 \
  bash "$prune_script" --apply > /dev/null 2>&1
delete_status=$?
set -e
[[ "$delete_status" == 9 && "$(wc -l < "$GH_CALLS" | tr -d ' ')" == 3 ]]

# Report budget pressure instead of deleting the protected working set.
jq -n '[{id:1,key:"v1-rust-static-Linux-x64-aaaaaaaa-22222222",ref:"refs/heads/main",
  createdAt:"2026-10-27T01:00:01Z",sizeInBytes:10000000001}]' > "$fixture_root/overbudget.json"
: > "$GH_CALLS"
if GITHUB_EVENT_NAME=push GITHUB_REF=refs/heads/main GITHUB_REPOSITORY=example/repo \
  GH_FIXTURE="$fixture_root/overbudget.json" bash "$prune_script" --apply > /dev/null 2>&1; then
  echo 'Over-budget working set unexpectedly succeeded' >&2
  exit 1
fi
[[ "$(wc -l < "$GH_CALLS" | tr -d ' ')" == 3 ]]

printf '%s\n' '{"jobs":[]}' > "$fixture_root/no-writers.json"
: > "$GH_CALLS"
if GITHUB_EVENT_NAME=push GITHUB_REF=refs/heads/main GITHUB_REPOSITORY=example/repo \
  GH_JOB_FIXTURE="$fixture_root/no-writers.json" bash "$prune_script" --apply > /dev/null 2>&1; then
  echo 'Apply without writer timestamps unexpectedly succeeded' >&2
  exit 1
fi
[[ "$(wc -l < "$GH_CALLS" | tr -d ' ')" == 1 ]]
printf 'Main cache pruning: 36 checks passed\n'
