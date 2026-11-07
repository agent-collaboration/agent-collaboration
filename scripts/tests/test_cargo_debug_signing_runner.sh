#!/usr/bin/env bash
# Exercise the cargo signing runner against fake codesign/security/uname tools:
# it signs the bin targets beside a test binary before running it, never runs
# an executable it cannot sign on a developer machine, and keeps stable
# identifiers for hyphenated bin names.
set -euo pipefail
repository_root="$(git rev-parse --show-toplevel)"
runner="$repository_root/scripts/cargo_debug_signing_runner.sh"
fixture_root="$(mktemp -d)"
trap 'rm -rf "$fixture_root"' EXIT

fake_tools="$fixture_root/bin"
mkdir -p "$fake_tools"

# uname: pretend to be macOS so the runner signs, on any CI host.
cat > "$fake_tools/uname" <<'EOF'
#!/usr/bin/env bash
echo Darwin
EOF
# security: one Developer ID identity for the team, unless FAKE_NO_IDENTITY.
cat > "$fake_tools/security" <<'EOF'
#!/usr/bin/env bash
if [[ -z "${FAKE_NO_IDENTITY:-}" ]]; then
  echo '  1) ABCDEF0123456789 "Developer ID Application: Test (974QD84WVC)"'
fi
echo '     1 valid identities found'
EOF
# codesign: appends a "# SIGNED <identifier>" comment to the file it signs and
# reports that identifier back for -dv.
cat > "$fake_tools/codesign" <<'EOF'
#!/usr/bin/env bash
set -euo pipefail
case "$1" in
  -dv)
    file="$2"
    if grep -q '^# SIGNED ' "$file" 2>/dev/null; then
      echo "Identifier=$(grep '^# SIGNED ' "$file" | tail -1 | cut -d' ' -f3)" >&2
      exit 0
    fi
    echo "Identifier=linker-signed" >&2
    exit 0
    ;;
  --verify) exit 0 ;;
  --force)
    identifier="" file=""
    while [[ $# -gt 0 ]]; do
      case "$1" in
        --identifier) identifier="$2"; shift 2 ;;
        --*) shift ;;
        *) file="$1"; shift ;;
      esac
    done
    echo "# SIGNED $identifier" >> "$file"
    echo "$identifier" >> "$FAKE_SIGN_LOG"
    ;;
esac
EOF
# find: the real one, or with FAKE_FIND_FAIL a partial listing then a failure.
real_find="$(command -v find)"
cat > "$fake_tools/find" <<EOF
#!/usr/bin/env bash
if [[ -n "\${FAKE_FIND_FAIL:-}" ]]; then
  printf '%s\\0' "\$1/codex-router"
  exit 1
fi
exec "$real_find" "\$@"
EOF
# mktemp: the real one, or with FAKE_MKTEMP_FAIL_NEXT a failure for the second
# sweep file only.
real_mktemp="$(command -v mktemp)"
cat > "$fake_tools/mktemp" <<EOF
#!/usr/bin/env bash
if [[ -n "\${FAKE_MKTEMP_FAIL_NEXT:-}" && "\$1" == *".next."* ]]; then
  echo "mktemp: no space" >&2
  exit 1
fi
exec "$real_mktemp" "\$@"
EOF
chmod +x "$fake_tools"/*

profile="$fixture_root/target/debug"
mkdir -p "$profile/deps"
make_executable() {
  printf '#!/usr/bin/env bash\necho ran %s\n' "$(basename "$1")" > "$1"
  chmod +x "$1"
}
make_executable "$profile/codex-router"
make_executable "$profile/codex-router-board-staging-cutover"
make_executable "$profile/deps/integration-0123456789abcdef"
printf 'not executable\n' > "$profile/codex-router.d"

export PATH="$fake_tools:$PATH" FAKE_SIGN_LOG="$fixture_root/sign.log"
unset GITHUB_ACTIONS || true

no_sweep_leftovers() {
  if ls "$profile"/.debug-signing-verified.* > /dev/null 2>&1; then
    echo "sweep left a temporary file" >&2
    exit 1
  fi
}

# 1. A test binary run through the runner signs the bins beside it first.
output="$("$runner" "$profile/deps/integration-0123456789abcdef")"
[[ "$output" == "ran integration-0123456789abcdef" ]]
grep -qx 'dev.shravansunder.codex-router.debug' "$FAKE_SIGN_LOG"
grep -qx 'dev.shravansunder.test.codex-router-board-staging-cutover' "$FAKE_SIGN_LOG"
grep -qx 'dev.shravansunder.test.integration' "$FAKE_SIGN_LOG"
[[ "$(wc -l < "$FAKE_SIGN_LOG" | tr -d ' ')" == 3 ]]
[[ -s "$profile/.debug-signing-verified" ]]
no_sweep_leftovers

# 2. A second launch with nothing rebuilt signs nothing new.
: > "$FAKE_SIGN_LOG"
"$runner" "$profile/deps/integration-0123456789abcdef" > /dev/null
[[ ! -s "$FAKE_SIGN_LOG" ]]

# 3. An unsigned build put back at a bin path with an OLD modification time
# (Cargo copies keep it) is still signed before the test runs.
make_executable "$fixture_root/saved-codex-router"
touch -t 200001010000 "$fixture_root/saved-codex-router"
cp -p "$fixture_root/saved-codex-router" "$profile/codex-router"
grep -q '^# SIGNED ' "$profile/codex-router" && { echo "fixture already signed" >&2; exit 1; }
"$runner" "$profile/deps/integration-0123456789abcdef" > /dev/null
grep -qx 'dev.shravansunder.codex-router.debug' "$FAKE_SIGN_LOG"
grep -q '^# SIGNED dev.shravansunder.codex-router.debug' "$profile/codex-router"

# 4. A failed or partial listing of the profile directory refuses to run.
: > "$FAKE_SIGN_LOG"
make_executable "$profile/agent-sessions"
if FAKE_FIND_FAIL=1 "$runner" "$profile/deps/integration-0123456789abcdef" > "$fixture_root/scan.out" 2> "$fixture_root/scan.err"; then
  echo "runner ran after a failed scan" >&2
  exit 1
fi
grep -q 'could not list its executables' "$fixture_root/scan.err"
[[ ! -s "$fixture_root/scan.out" ]]
grep -q '^# SIGNED ' "$profile/agent-sessions" && { echo "partial scan signed past its failure" >&2; exit 1; }
no_sweep_leftovers

# 5. Without a signing identity, a developer machine refuses to run anything,
# and leaves no sweep files behind.
make_executable "$profile/deps/integration-fedcba9876543210"
if FAKE_NO_IDENTITY=1 "$runner" "$profile/deps/integration-fedcba9876543210" > "$fixture_root/refused.out" 2> "$fixture_root/refused.err"; then
  echo "runner ran an unsigned executable" >&2
  exit 1
fi
grep -q 'refusing to run unsigned agent-sessions' "$fixture_root/refused.err"
[[ ! -s "$fixture_root/refused.out" ]]
no_sweep_leftovers
"$runner" --sign-only "$profile/agent-sessions"
if FAKE_NO_IDENTITY=1 "$runner" "$profile/deps/integration-fedcba9876543210" > "$fixture_root/refused.out" 2> "$fixture_root/refused.err"; then
  echo "runner ran an unsigned executable" >&2
  exit 1
fi
grep -q 'refusing to run unsigned integration-fedcba9876543210' "$fixture_root/refused.err"
[[ ! -s "$fixture_root/refused.out" ]]
if FAKE_NO_IDENTITY=1 "$runner" --sign-only "$profile/deps/integration-fedcba9876543210" 2> /dev/null; then
  echo "--sign-only reported success without signing" >&2
  exit 1
fi

# 6. A symlinked executable at the top of the profile directory is still
# checked: refused without an identity, signed before it runs with one.
make_executable "$fixture_root/linked-target"
ln -s "$fixture_root/linked-target" "$profile/linked-tool"
if FAKE_NO_IDENTITY=1 "$runner" "$profile/linked-tool" > "$fixture_root/link.out" 2> "$fixture_root/link.err"; then
  echo "runner ran an unsigned symlinked executable" >&2
  exit 1
fi
[[ ! -s "$fixture_root/link.out" ]]
output="$("$runner" "$profile/linked-tool")"
[[ "$output" == "ran linked-target" ]]
grep -q '^# SIGNED ' "$profile/linked-tool"
no_sweep_leftovers

# 6b. A requested executable whose name the scan skips is still checked.
make_executable "$profile/tool.signing.saved"
if FAKE_NO_IDENTITY=1 "$runner" "$profile/tool.signing.saved" > "$fixture_root/named.out" 2> /dev/null; then
  echo "runner ran an unsigned executable the scan skips" >&2
  exit 1
fi
[[ ! -s "$fixture_root/named.out" ]]
no_sweep_leftovers

# 7. A failure creating the second sweep file refuses and leaves no file.
if FAKE_MKTEMP_FAIL_NEXT=1 "$runner" "$profile/deps/integration-0123456789abcdef" > "$fixture_root/mk.out" 2> /dev/null; then
  echo "runner ran after a failed sweep allocation" >&2
  exit 1
fi
[[ ! -s "$fixture_root/mk.out" ]]
no_sweep_leftovers

# 8. In GitHub Actions the same build runs linker-signed with a warning.
output="$(GITHUB_ACTIONS=true FAKE_NO_IDENTITY=1 "$runner" "$profile/deps/integration-fedcba9876543210" 2> "$fixture_root/ci.err")"
[[ "$output" == "ran integration-fedcba9876543210" ]]
grep -q 'stays linker-signed' "$fixture_root/ci.err"

echo "cargo debug signing runner: all checks passed"
