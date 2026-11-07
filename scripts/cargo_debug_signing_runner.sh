#!/usr/bin/env bash
# Cargo runner on Apple Silicon: signs every executable it runs with the debug
# Developer ID before running it, then runs it unchanged.
#
# Keychain approvals belong to a code identity. Linker-signed builds are
# identified by their hash, so each rebuild asks for Keychain access again.
# A Developer ID signature with a fixed identifier keeps one identity across
# rebuilds. The ".debug" identifiers keep debug builds a different identity
# from released builds, so a debug build never inherits approvals given to
# the production Router's Keychain item.
#
# Test binaries, examples and helper executables are signed too. macOS
# verifies each new ad-hoc-signed executable it launches, and nextest launches
# every test binary (to list it, then once per test), so unsigned test builds
# trigger repeated XProtect/syspolicyd verification. They get a per-binary
# "dev.shravansunder.test.<name>" identifier.
#
# Usage: as the cargo runner, `<executable> [arguments...]`; or
# `--sign-only <executable>...` to sign builds another process starts directly,
# such as the debug Router a debug-host example launches, or the
# board-debug-runtime Host that end-to-end tests start.
#
# Tests also launch bin targets directly (env!("CARGO_BIN_EXE_<name>"), often
# from a copy), bypassing the runner. So before running any executable the
# runner also signs every executable at the top of its profile directory
# (target/<profile>/), the files CARGO_BIN_EXE_* names. A copy keeps the
# signature.
#
# On a developer Mac the runner fails closed: an executable it cannot sign is
# never run, because every unsigned launch makes macOS verify it again and
# raises security prompts. In GitHub Actions (no prompts, maybe no
# certificate) it warns and runs the build linker-signed.
set -euo pipefail

if [[ "$(uname -s)" != Darwin ]]; then
  if [[ "${1:-}" == --sign-only ]]; then
    exit 0
  fi
  exec "$@"
fi

readonly DEVELOPER_ID_TEAM="974QD84WVC"

debug_identifier_for() {
  case "$(basename "$1")" in
    codex-router | agent-collaboration | agent-sessions | agent-router-update | board-debug-runtime)
      echo "dev.shravansunder.$(basename "$1").debug"
      ;;
    *)
      # Cargo test and example artifacts end in "-<16 hex digits>"; keep the
      # stable name. Bin targets have no hash and may contain "-".
      local name
      name="$(basename "$1")"
      if [[ "${name}" =~ ^(.+)-[0-9a-f]{16}$ ]]; then
        name="${BASH_REMATCH[1]}"
      fi
      echo "dev.shravansunder.test.${name}"
      ;;
  esac
}

warn_unsigned() {
  if [[ "${GITHUB_ACTIONS:-}" == true ]]; then
    echo "cargo debug runner: $(basename "$1") stays linker-signed ($2)" >&2
    return 0
  fi
  echo "cargo debug runner: refusing to run unsigned $(basename "$1"): $2. Fix signing (login keychain unlocked, Developer ID for team ${DEVELOPER_ID_TEAM}); never run it unsigned." >&2
  exit 1
}

has_debug_identity() {
  local signature
  signature="$(codesign -dv "$1" 2>&1)" || return 1
  grep -qx "Identifier=$2" <<<"${signature}" &&
    codesign --verify "$1" 2>/dev/null
}

sign_with_debug_identity() {
  local executable="$1"
  local debug_identifier="$2"
  local signing_identity signing_output

  if has_debug_identity "${executable}" "${debug_identifier}"; then
    return 0
  fi

  signing_identity="$(
    security find-identity -v -p codesigning |
      awk -v team="(${DEVELOPER_ID_TEAM})\"" 'index($0, "Developer ID Application") && index($0, team) { print $2; exit }'
  )"
  if [[ -z "${signing_identity}" ]]; then
    warn_unsigned "${executable}" "no Developer ID identity for team ${DEVELOPER_ID_TEAM}"
    return 0
  fi

  # nextest launches one binary several times at once (to list it, then per
  # test). Re-signing the file in place while another launch is executing it
  # invalidates that process's code signature and macOS kills it (SIGKILL,
  # no output). Sign a private copy instead and rename it over the original:
  # the rename is atomic, a process already running keeps the old file, and
  # concurrent signers each install a complete signed file.
  # Any failure (no space for the copy, codesign, the rename) removes the copy
  # and leaves the original in place, which then runs linker-signed.
  local signed_copy
  if ! signed_copy="$(mktemp "${executable}.signing.XXXXXX" 2>&1)"; then
    warn_unsigned "${executable}" "could not create a private copy: ${signed_copy}"
    return 0
  fi
  if ! signing_output="$(cp -p "${executable}" "${signed_copy}" 2>&1)"; then
    rm -f "${signed_copy}"
    warn_unsigned "${executable}" "could not copy for signing: ${signing_output}"
    return 0
  fi
  # Debug builds skip Apple's timestamp server so signing works offline.
  if ! signing_output="$(codesign --force --sign "${signing_identity}" --identifier "${debug_identifier}" --timestamp=none "${signed_copy}" 2>&1)"; then
    rm -f "${signed_copy}"
    warn_unsigned "${executable}" "codesign failed: ${signing_output}"
    return 0
  fi
  if ! signing_output="$(mv -f "${signed_copy}" "${executable}" 2>&1)"; then
    rm -f "${signed_copy}"
    warn_unsigned "${executable}" "could not install the signed copy: ${signing_output}"
  fi
}

# Bin targets sit directly in the profile directory; test binaries in its
# deps/ subdirectory and examples in examples/.
profile_directory_for() {
  local directory
  directory="$(cd "$(dirname "$1")" && pwd)"
  case "$(basename "${directory}")" in
    deps | examples) dirname "${directory}" ;;
    *) echo "${directory}" ;;
  esac
}

# Sign every executable at the top of the profile directory that changed since
# the last sweep (all of them on the first), so a test that launches
# CARGO_BIN_EXE_<name> directly, or a copy of it, starts a signed build. A
# stamp file keeps repeated launches (nextest runs each test binary many
# times) to a find over new files.
sign_profile_bins() {
  local profile_directory="$1"
  local stamp="${profile_directory}/.debug-signing-checked"
  local candidate
  # Global, so the EXIT trap can still see it after a refusal exits.
  if ! sweep_stamp="$(mktemp "${stamp}.XXXXXX" 2>&1)"; then
    warn_unsigned "${profile_directory}" "could not create the sweep stamp: ${sweep_stamp}"
    return 0
  fi
  # A refusal exits the runner; don't leave the new stamp behind.
  trap 'rm -f "${sweep_stamp}"' EXIT
  local newer_than=()
  if [[ -e "${stamp}" ]]; then
    newer_than=(-newer "${stamp}")
  fi
  while IFS= read -r -d '' candidate; do
    sign_with_debug_identity "${candidate}" "$(debug_identifier_for "${candidate}")"
  done < <(find "${profile_directory}" -maxdepth 1 -type f -perm -u+x ! -name '*.dylib' ! -name '*.signing.*' ${newer_than[@]+"${newer_than[@]}"} -print0)
  mv -f "${sweep_stamp}" "${stamp}"
  trap - EXIT
}

if [[ "${1:-}" == "--sign-only" ]]; then
  shift
  for executable in "$@"; do
    sign_with_debug_identity "${executable}" "$(debug_identifier_for "${executable}")"
  done
  exit 0
fi

executable="$1"
shift
sign_profile_bins "$(profile_directory_for "${executable}")"
sign_with_debug_identity "${executable}" "$(debug_identifier_for "${executable}")"
exec "${executable}" "$@"
