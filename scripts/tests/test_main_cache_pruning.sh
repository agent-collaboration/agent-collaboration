#!/usr/bin/env bash
# Exercise deletion selection and the real CLI wrapper without deleting caches.
set -euo pipefail
repository_root="$(git rev-parse --show-toplevel)"
prune_script="$repository_root/scripts/tooling/prune-main-caches.sh"
fixture_root="$(mktemp -d)"
trap 'rm -rf "$fixture_root"' EXIT
writers_started_at=2026-10-27T01:00:00Z

jq -n '
  ["v0-rust-static-", "v0-rust-test-", "v0-rust-e2e-", "v0-rust-wasm-",
   "v0-rust-release-arm64-background-", "Linux-X64-rust-tools-",
   "Linux-X64-rust-tools-sqlx-", "Linux-X64-worker-build-"]
  | to_entries | map(
      [{id: (100 + .key), key: (.value + "z-old"), ref: "refs/heads/main",
        createdAt: "2026-10-26T01:00:00Z", sizeInBytes: 10},
       {id: (200 + .key), key: (.value + "a-new"), ref: "refs/heads/main",
        createdAt: "2026-10-27T01:00:01.2Z", sizeInBytes: 20}]) | add
  | . + [{id: 900, key: "v0-rust-static-pr", ref: "refs/pull/1/merge",
          createdAt: "2026-10-27T01:00:02Z", sizeInBytes: 30},
         {id: 901, key: "v0-rust-static-tag", ref: "refs/tags/v1.0.0",
          createdAt: "2026-10-26T01:00:00Z", sizeInBytes: 40},
         {id: 902, key: "v0-rust-staticish-other", ref: "refs/heads/main",
          createdAt: "2026-10-26T01:00:00Z", sizeInBytes: 50}]
  | reverse
' > "$fixture_root/inventory.json"

bash "$prune_script" --plan "$writers_started_at" < "$fixture_root/inventory.json" > "$fixture_root/plan.json"
jq -e 'length == 8 and ([.[].delete[].id] | sort) == [100,101,102,103,104,105,106,107]
  and all(.[]; .keepKey | endswith("a-new"))' "$fixture_root/plan.json" > /dev/null

# An exact hit has only one key and creates no new archive.
jq 'map(select(.id >= 200))' "$fixture_root/inventory.json" > "$fixture_root/hits.json"
bash "$prune_script" --plan "$writers_started_at" < "$fixture_root/hits.json" | jq -e '[.[].delete[]] == []' > /dev/null
# Never delete an older cache version under the protected key either.
jq '. + [(.[] | select(.id == 200)) + {id: 903, createdAt: "2026-10-26T00:00:00Z"}]' "$fixture_root/inventory.json" > "$fixture_root/versions.json"
bash "$prune_script" --plan "$writers_started_at" < "$fixture_root/versions.json" | jq -e '([.[].delete[].id] | sort) == [100,101,102,103,104,105,106,107]' > /dev/null

# Compare fractional seconds accurately at the writer-start boundary.
printf '%s\n' '[
  {"id":1,"key":"v0-rust-static-old","ref":"refs/heads/main","createdAt":"2026-10-27T01:00:00.09Z"},
  {"id":2,"key":"v0-rust-static-new","ref":"refs/heads/main","createdAt":"2026-10-27T01:00:00.2Z"}
]' | bash "$prune_script" --plan 2026-10-27T01:00:00.100000001Z | jq -e '.[0].keepKey == "v0-rust-static-new" and [.[].delete[].id] == [1]' > /dev/null

# Unknown winners, multiple saves, invalid IDs and truncated inventories fail closed.
expect_plan_failure() {
  if bash "$prune_script" --plan "$writers_started_at" < "$1" > /dev/null 2>&1; then
    echo "Expected selection to reject $2" >&2
    exit 1
  fi
}
jq 'map(.createdAt = "2026-10-26T01:00:00Z")' "$fixture_root/inventory.json" > "$fixture_root/ambiguous.json"
expect_plan_failure "$fixture_root/ambiguous.json" 'an unidentifiable saved key'
jq 'map(.createdAt = "2026-10-27T01:00:01Z")' "$fixture_root/inventory.json" > "$fixture_root/multiple.json"
expect_plan_failure "$fixture_root/multiple.json" 'multiple saved keys'
jq '.[0].id = "not-an-id"' "$fixture_root/inventory.json" > "$fixture_root/invalid.json"
expect_plan_failure "$fixture_root/invalid.json" 'an invalid ID'
jq '.[0].id = .[1].id' "$fixture_root/inventory.json" > "$fixture_root/duplicate.json"
expect_plan_failure "$fixture_root/duplicate.json" 'duplicate IDs'
jq -n '[range(1000) | {id: (1 + .), key: "unrelated", ref: "refs/heads/main"}]' > "$fixture_root/truncated.json"
expect_plan_failure "$fixture_root/truncated.json" 'a possibly truncated inventory'

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
  {"name":"Validate Rust workspace","startedAt":"0001-01-01T00:00:00Z","conclusion":null},
  {"name":"Prune superseded main caches","startedAt":"2026-10-27T02:00:00Z","conclusion":null},
  {"name":"Static validation","startedAt":"2026-10-27T01:00:00Z","conclusion":"success"},
  {"name":"Test Rust workspace","startedAt":"2026-10-27T01:00:03Z","conclusion":"failure"},
  {"name":"Warm Apple Silicon release dependencies","startedAt":"0001-01-01T00:00:00Z","conclusion":"skipped"}
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
awk '$1 == "cache" && $2 == "delete" {print $3}' "$GH_CALLS" | jq -s -e 'sort == [100,101,102,103,104,105,106,107]' > /dev/null
[[ "$(wc -l < "$GH_CALLS" | tr -d ' ')" == 11 ]]

: > "$GH_CALLS"
if GITHUB_EVENT_NAME=push GITHUB_REF=refs/heads/main GITHUB_REPOSITORY=example/repo \
  GH_FIXTURE="$fixture_root/ambiguous.json" bash "$prune_script" --apply > /dev/null 2>&1; then
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
jq -n '[{id:1,key:"v0-rust-static-current",ref:"refs/heads/main",
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
printf 'Main cache pruning: 17 checks passed\n'
