#!/usr/bin/env bash
# Print test durations from nextest and unittest logs, including gh escaping.
set -euo pipefail
job_name="${1:?job name is required}"
shift
awk -v job="$job_name" '
# Parse nextest statuses and Python unittest --durations 0 output.
{
  gsub(/\033\[[0-9;]*m/, "")
  # Newer gh versions render ESC as the printable pair ^[.
  gsub(/\^\[\[[0-9;]*m/, "")
  if (match($0, /(PASS|FAIL|SLOW|TIMEOUT) +\[ *[0-9.]+s\]/)) {
    result = substr($0, RSTART, RLENGTH)
    duration = result
    sub(/^[^[]*\[ */, "", duration)
    sub(/s\]$/, "", duration)
    name = substr($0, RSTART + RLENGTH)
    sub(/^ +/, "", name)
    sub(/^\( *[0-9]+\/[0-9]+\) +/, "", name)
    status = result
    sub(/ .*/, "", status)
    print duration "\t" status "\t" job "\t" name
  } else if (match($0, /TRANSPORT_TEST\t[0-9]+\t[0-9]+\t/)) {
    split(substr($0, RSTART), fields, "\t")
    status = fields[3] == 0 ? "PASS" : "FAIL"
    print fields[2] "\t" status "\t" job "\t" fields[4]
  } else if (match($0, /[0-9]+\.[0-9]+s test_[^ ]+/)) {
    result = substr($0, RSTART)
    duration = result
    sub(/s .*/, "", duration)
    sub(/^[0-9.]+s /, "", result)
    print duration "\tMEASURED\t" job "\t" result
  }
}
' "$@"
