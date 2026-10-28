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
# Signing is a convenience: when it cannot happen the executable still runs,
# linker-signed, and Keychain access prompts again after the next rebuild.
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
      # Cargo test and example artifacts end in "-<hash>"; keep the stable name.
      local name
      name="$(basename "$1")"
      echo "dev.shravansunder.test.${name%-*}"
      ;;
  esac
}

warn_unsigned() {
  echo "cargo debug runner: $(basename "$1") stays linker-signed ($2); Keychain access prompts after each rebuild" >&2
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

if [[ "${1:-}" == "--sign-only" ]]; then
  shift
  for executable in "$@"; do
    sign_with_debug_identity "${executable}" "$(debug_identifier_for "${executable}")"
  done
  exit 0
fi

executable="$1"
shift
sign_with_debug_identity "${executable}" "$(debug_identifier_for "${executable}")"
exec "${executable}" "$@"
