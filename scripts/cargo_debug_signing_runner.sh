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
    codex-router | agent-collaboration | agent-sessions | codex-router-board-upgrade | board-debug-runtime)
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

# A file's identity for the verified list: its inode and inode-change time,
# to the nanosecond (an in-place overwrite keeps the inode).
# Cargo can install an older unsigned build with its original modification
# time (copies keep it), but any new file at the path gets a new inode and a
# new change time, which userland can't set back.
file_identity() {
  if stat --version > /dev/null 2>&1; then
    stat -c '%i:%z' "$1"
  else
    stat -f '%i:%Fc' "$1"
  fi
}

# Sign every executable at the top of the profile directory, so a test that
# launches CARGO_BIN_EXE_<name> directly, or a copy of it, starts a signed
# build. A list of files already verified (path, inode, change time) keeps
# repeated launches (nextest runs each test binary many times) to a stat per
# file; any file not on the list as it is now is checked and signed.
sign_profile_bins() {
  local profile_directory="$1"
  # Optional: the executable about to run, checked against the same list.
  local requested="${2:-}"
  local verified="${profile_directory}/.debug-signing-verified"
  local candidate identity
  # Globals, so the EXIT trap can still see them after a refusal exits.
  if ! sweep_listing="$(mktemp "${verified}.listing.XXXXXX" 2>&1)" ||
    ! sweep_verified="$(mktemp "${verified}.next.XXXXXX" 2>&1)"; then
    warn_unsigned "${profile_directory}" "could not create sweep files"
    return 0
  fi
  # A refusal exits the runner; don't leave sweep files behind.
  trap 'rm -f "${sweep_listing}" "${sweep_verified}"' EXIT
  # Enumerate into a file so a failed or partial scan is a refusal, not a
  # silently shorter list.
  if ! find "${profile_directory}" -maxdepth 1 -type f -perm -u+x ! -name '*.dylib' ! -name '*.signing.*' -print0 > "${sweep_listing}"; then
    warn_unsigned "${profile_directory}" "could not list its executables"
    return 0
  fi
  if [[ -n "${requested}" && "$(dirname "${requested}")" != "${profile_directory}" ]]; then
    printf '%s\0' "${requested}" >> "${sweep_listing}"
  fi
  while IFS= read -r -d '' candidate; do
    identity="$(file_identity "${candidate}")"
    if [[ -f "${verified}" ]] && grep -Fqx -- "${identity} ${candidate}" "${verified}"; then
      echo "${identity} ${candidate}" >> "${sweep_verified}"
      continue
    fi
    sign_with_debug_identity "${candidate}" "$(debug_identifier_for "${candidate}")"
    if has_debug_identity "${candidate}" "$(debug_identifier_for "${candidate}")"; then
      echo "$(file_identity "${candidate}") ${candidate}" >> "${sweep_verified}"
    fi
  done < "${sweep_listing}"
  mv -f "${sweep_verified}" "${verified}"
  rm -f "${sweep_listing}"
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
# The sweep also checks (and signs) the executable itself, so an unchanged
# build costs a stat, not a full signature verification, on every launch.
sign_profile_bins "$(profile_directory_for "${executable}")" "$(cd "$(dirname "${executable}")" && pwd)/$(basename "${executable}")"
exec "${executable}" "$@"
