#!/usr/bin/env bash
# Fetch exact GitHub step timings and test durations from a completed run.
set -euo pipefail

run_id="${1:?usage: report-workflow-timings.sh RUN_ID OUTPUT_DIRECTORY}"
output_directory="${2:?output directory is required}"
repository="${GITHUB_REPOSITORY:-$(gh repo view --json nameWithOwner --jq .nameWithOwner)}"
mkdir -p "$output_directory"
gh api --paginate --slurp "repos/$repository/actions/runs/$run_id/jobs?filter=latest&per_page=100" > "$output_directory/jobs.json"
jq -r '
  .[].jobs[] | .name as $job | .steps[] |
  select(.started_at != null and .completed_at != null) |
  [((.completed_at | fromdateiso8601) - (.started_at | fromdateiso8601)), .conclusion, $job, .name] | @tsv
' "$output_directory/jobs.json" | sort -nr > "$output_directory/steps.tsv"

: > "$output_directory/tests.tsv"
while IFS=$'\t' read -r job_id job_name; do
  # A final report step can run while its own job is still in progress.
  # GitHub serves logs only for finished jobs; the CLI also supports a later
  # complete-run invocation so post-action and report steps are included.
  gh run view "$run_id" --repo "$repository" --job "$job_id" --log > "$output_directory/job-$job_id.log"
  bash "$(dirname "$0")/extract-test-timings.sh" "$job_name" "$output_directory/job-$job_id.log" >> "$output_directory/tests.tsv"
done < <(jq -r '.[].jobs[] | select(.status == "completed") | [.id, .name] | @tsv' "$output_directory/jobs.json")
sort -nr -u "$output_directory/tests.tsv" > "$output_directory/tests-sorted.tsv"
{
  printf '## Slowest workflow steps\n\n| Seconds | Result | Job | Step |\n| ---: | --- | --- | --- |\n'
  head -20 "$output_directory/steps.tsv" | awk -F '\t' '{print "| " $1 " | " $2 " | " $3 " | " $4 " |"}'
  printf '\n## Slowest tests\n\n| Seconds | Result | Job | Test |\n| ---: | --- | --- | --- |\n'
  head -20 "$output_directory/tests-sorted.tsv" | awk -F '\t' '{print "| " $1 " | " $2 " | " $3 " | " $4 " |"}'
  printf '\nFull step and test inventories are in the timing artifact. Test rows come from nextest statuses and unittest durations; skipped tests have no execution duration. Overlapping step durations must not be added.\n'
} > "$output_directory/summary.md"
cat "$output_directory/summary.md"
if [[ -n "${GITHUB_STEP_SUMMARY:-}" ]]; then
  cat "$output_directory/summary.md" >> "$GITHUB_STEP_SUMMARY"
fi
