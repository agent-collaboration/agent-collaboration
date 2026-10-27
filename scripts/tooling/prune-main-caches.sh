#!/usr/bin/env bash
# Keep the key saved by this serialized main run, then remove its predecessors.
set -euo pipefail

prune_mode="${1:-}"
if [[ ( "$prune_mode" != --plan || $# != 2 ) && ( "$prune_mode" != --apply || $# != 1 ) ]]; then
  echo 'Usage: prune-main-caches.sh --plan <first-cache-writer-started-at> | --apply' >&2
  exit 2
fi

if [[ "$prune_mode" == --apply ]]; then
  if [[ "${GITHUB_EVENT_NAME:-}" != push || "${GITHUB_REF:-}" != refs/heads/main ]]; then
    echo 'Cache deletion is allowed only on a push to main.' >&2
    exit 2
  fi
  : "${GITHUB_REPOSITORY:?GITHUB_REPOSITORY is required}"
  : "${GITHUB_RUN_ID:?GITHUB_RUN_ID is required}"
  : "${GITHUB_RUN_ATTEMPT:?GITHUB_RUN_ATTEMPT is required}"
  # Run creation may predate another main run's final save while queued.
  # Only actual writer-job starts are inside this run's concurrency slot.
  cache_jobs="$(gh run view "$GITHUB_RUN_ID" --repo "$GITHUB_REPOSITORY" \
    --attempt "$GITHUB_RUN_ATTEMPT" --json jobs)"
  writers_started_at="$(printf '%s\n' "$cache_jobs" | jq -er '
    [.jobs[] | select(.name == "Static validation" or .name == "Test Rust workspace"
      or .name == "Rust local end-to-end" or .name == "Central Worker build and local workerd conformance"
      or .name == "Warm Apple Silicon release dependencies")
      | select(.conclusion != "skipped" and .startedAt != null and .startedAt != "0001-01-01T00:00:00Z")
      | .startedAt] | min // error("No cache writer start time")')"
  cache_inventory="$(gh cache list --repo "$GITHUB_REPOSITORY" \
    --ref refs/heads/main --limit 1000 --json id,key,ref,createdAt,sizeInBytes)"
else
  # --plan accepts the same JSON as gh cache list, without any API mutations.
  writers_started_at="$2"
  cache_inventory="$(cat)"
fi

# rust-cache exposes no primary-key output. Main runs are serialized by the
# workflow concurrency group, and this job waits for every writer's post hook.
# A key created since the first writer started therefore belongs to this run.
# Cache hits create no entry: a single existing key needs no pruning. Refuse an
# ambiguous family rather than guess from last-access order or delete its key.
cache_plan="$(printf '%s\n' "$cache_inventory" | jq -e --arg since "$writers_started_at" '
  def timestamp:
    (capture("^(?<whole>[0-9]{4}-[0-9]{2}-[0-9]{2}T[0-9]{2}:[0-9]{2}:[0-9]{2})(?:\\.(?<fraction>[0-9]{1,9}))?Z$")
      // error("Expected a UTC cache-writer timestamp"))
    | .whole + "." + (((.fraction // "") + "000000000")[0:9]) + "Z";
  def family($key):
    ["Linux-X64-rust-tools-sqlx-", "Linux-X64-rust-tools-",
     "Linux-X64-worker-build-", "v0-rust-static-", "v0-rust-test-",
     "v0-rust-e2e-", "v0-rust-wasm-", "v0-rust-release-arm64-background-"]
    | map(. as $prefix | select($key | startswith($prefix))) | first;
  ($since | timestamp) as $started
  | if type != "array" or length >= 1000 then
      error("Cache inventory must be a complete array with fewer than 1000 entries")
    else . end
  | if all(.[]; (.id | type == "number" and . > 0 and floor == .)
      and (.key | type == "string") and (.ref | type == "string"))
      and ([.[].id] | unique | length) == length then .
    else error("Invalid or duplicate cache identity") end
  | map(select(.ref == "refs/heads/main") | . + {family: family(.key)})
  | map(select(.family != null))
  | group_by(.family)
  | map(
      . as $entries
      | (map(.key) | unique) as $keys
      | (map(select((.createdAt | timestamp) >= $started) | .key) | unique) as $saved
      | (if ($keys | length) == 1 then $keys[0]
         elif ($saved | length) == 1 then $saved[0]
         else error("Cannot identify the saved key for " + .[0].family) end) as $keep
      | {family: .[0].family, keepKey: $keep,
         delete: [$entries[] | select(.key != $keep) | {id, key, sizeInBytes}]})
')"

# Selection must succeed for the entire inventory before any delete is issued.
printf '%s\n' "$cache_plan"
if [[ "$prune_mode" == --apply ]]; then
  while IFS= read -r cache_id; do
    gh cache delete "$cache_id" --repo "$GITHUB_REPOSITORY"
  done < <(printf '%s\n' "$cache_plan" | jq -r '.[].delete[].id')
  # Include other refs in the budget check, without deleting their caches.
  gh cache list --repo "$GITHUB_REPOSITORY" --limit 1000 --json sizeInBytes \
    | jq -e 'if length >= 1000 then error("Cache budget inventory may be truncated")
        else (map(.sizeInBytes) | add // 0) as $bytes
        | if $bytes <= 10000000000 then {cacheBytes: $bytes, budgetBytes: 10000000000}
          else error("Repository cache usage exceeds the 10 GB budget: " + ($bytes | tostring)) end end'
fi
