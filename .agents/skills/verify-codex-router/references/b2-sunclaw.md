# B2 central on Sunclaw

This reference owns the observed test-agent execution procedure, not permission
to operate a machine. Given an authorized declared target, a named test agent,
isolated source/evidence roots and an exclusive build slot, return qualified
tools, source guards, signed artifacts, literal outcomes and setup exceptions.
All commands below run on the test agent's machine from its declared clone root.

## Send one execution packet

Use the supported agent-collaboration route to the exact current test-agent
SessionRef. Do not copy an old session ID or use SSH as a substitute. The packet
names the exact source SHA, clean real-Git clone, build slot, evidence directory,
tool versions, required commands in order, expected outcomes, source/metadata
guards, owned-process cleanup and mandatory stops. Send once; queued custody is
not execution. Require an actual ACK naming the clone and phase before relying
on a run. Reconcile an uncertain delivery before sending another packet.

Run serially on the clone's normal Cargo target. Before a phase change, settle
all writers, freeze the old HEAD/source guard and name the new HEAD explicitly.
An approved doc-only successor can inherit unaffected observations only with
the complete diff and the 🦁 Lead's mapping; keep their original source labels.
New runtime/build/signature hashes name the actual successor. Preserve failed
commands, setup probes and their corrected qualifiers as separate records.

## Qualify Rust through the installed manager

The observed Sunclaw home Cargo bin lacked `cargo-fmt` and `cargo-clippy`
proxies despite installed components. Merely prepending the toolchain's `bin/`
made fmt discoverable but bypassed Rustup's loader environment: rust-lld could
not load `@rpath/libLLVM.dylib`. Treat the missing proxies as a host setup gap
for the test-agent owner; do not repair global Cargo bin during verification.

While that gap exists, use four task-private proxy links to the **same installed
Rustup executable**, and run commands through its pinned-toolchain route:

```sh
original_task_path="$PATH"
rustup_manager="$(realpath "$(command -v rustup)")"
private_proxy_bin="$EVIDENCE/rustup-proxy-bin"
mkdir "$private_proxy_bin"
chmod 700 "$private_proxy_bin"
for proxy_name in cargo rustc cargo-fmt cargo-clippy; do
  ln -s "$rustup_manager" "$private_proxy_bin/$proxy_name"
done
export PATH="$private_proxy_bin:$original_task_path"
```

`EVIDENCE` is the already-created owner-private directory from the packet.
Record the installed manager's canonical path/hash, each link target and the
unchanged original task PATH before any build. Do not overwrite existing proxy
files or prepend the toolchain's direct `bin/` directory. Resolve the pinned
installed toolchain to `1.98.1-aarch64-apple-darwin` on this Apple Silicon host.
The observed outer command route is:

```sh
mise exec node@26.11.1 -- "$rustup_manager" run 1.98.1-aarch64-apple-darwin cargo --version
mise exec node@26.11.1 -- "$rustup_manager" run 1.98.1-aarch64-apple-darwin rustc --version
mise exec node@26.11.1 -- "$rustup_manager" run 1.98.1-aarch64-apple-darwin cargo fmt --version
mise exec node@26.11.1 -- "$rustup_manager" run 1.98.1-aarch64-apple-darwin cargo clippy --version
```

Qualify all four inside the same environment as the gates: actual command
resolution, versions, sysroot, installed rustfmt/Clippy components and hashes,
and Rustup's standard loader environment containing the pinned toolchain's
library directory. If qualification fails, stop before building and return the
literal probe. Do not set DYLD variables manually, edit Mach-O/RPATHs, select an
alternate linker, install another toolchain or change global configuration.

Use that same outer route for the selected Cargo, worker-build, Node guard and
signing commands in [B2 central](b2-central.md). After an authorized environment
correction, qualify fmt and Tokio Clippy before the one corrected failed build:

```sh
mise exec node@26.11.1 -- "$rustup_manager" run 1.98.1-aarch64-apple-darwin cargo fmt --all -- --check
mise exec node@26.11.1 -- "$rustup_manager" run 1.98.1-aarch64-apple-darwin cargo clippy --locked -p collaboration-central-tokio --features test-support --all-targets -- -D warnings
```

Do not repeat already passed suites without a change, failure or unresolved
concern. A source diagnostic goes to the Implementer; a setup correction needs
the bounded corrective procedure before execution.

## Signed task-private SQLx 0.9.0

`scripts/tooling/bootstrap-tools.sh` executes version probes and can install a
missing tool. On this developer Mac it must not be the first thing to execute
fresh unsigned CLI files. If tools are already present, qualify their exact
version/signature/hash instead of force-reinstalling them. Otherwise, in the
isolated clone install exactly once into its task-private `tmp/rust-tools`, then
sign **both** executables before any probe:

```sh
cargo install sqlx-cli --version =0.9.0 --locked --root "$PWD/tmp/rust-tools" --force --no-default-features --features sqlite,rustls
scripts/cargo_debug_signing_runner.sh --sign-only tmp/rust-tools/bin/cargo-sqlx tmp/rust-tools/bin/sqlx
```

On the proxy-qualified route these commands also use the outer Rustup command
above. For either existing or newly signed tools, verify before bootstrap:

```sh
codesign -v tmp/rust-tools/bin/cargo-sqlx tmp/rust-tools/bin/sqlx
codesign -dv --verbose=2 tmp/rust-tools/bin/cargo-sqlx
codesign -dv --verbose=2 tmp/rust-tools/bin/sqlx
scripts/tooling/bootstrap-tools.sh sqlx --check
scripts/tooling/bootstrap-tools.sh sqlx
```

Require every exit zero, Developer ID Application authority and the
project's TeamIdentifier `974QD84WVC`. Retain identity metadata privately, not
secret values. Freeze both signed CLI hashes before bootstrap and require them
unchanged afterward: the non-check bootstrap must be a version-only no-op,
not an unnoticed reinstall/replacement.

Before metadata checks, freeze path list, bytes, SHA256 and modes for every
query JSON in root `.sqlx` and package `.sqlx` directories, plus the full tracked
source manifest. The accepted collection covered 402 metadata files; recompute
the inventory for a later declared target. These existing `--check` programs
describe queries and rewrite metadata during their round trip; they are not
filesystem read-only. Their package cleans also require the exclusive build slot.

```sh
node verification/tooling/prepare-sqlx.ts --check
node verification/tooling/prepare-sqlx-turso.ts --check
```

Run through the qualified Node/Rustup environment. Keep the clone/evidence
parents private. Scope umask 022 to the metadata subprocesses when needed to
preserve existing 0644 files; a blanket umask 077 changes their recreated modes.
After each check require zero exit and byte-, mode- and file-list-identical
metadata plus unchanged tracked source. A diff, install attempt, signature
failure or signing/Keychain prompt stops dependent work. Preserve the evidence;
do not restore/regenerate metadata or rerun without an authorized correction.

## Module-cycle ratchet

With cargo-modules **0.28.0**, preserve both the ordinary architecture suite and
the separate ignored scanner gate. Run the actual whole-workspace scan:

```sh
cargo nextest run -p workspace-architecture-check --run-ignored only
```

`architecture/README.md` and `crates/workspace-architecture-check/` own its
meaning: default-feature native library/bin-only targets, folded top-level SCC
members and edges inside or entering each SCC. It is a best-effort resolved-item
graph, not all feature/test/example configurations or every possible Rust path.
Ordinary architecture tests passing does not run this ignored gate.

When the scan reports a reduction, the authorized baseline owner first checks
the fresh scan has **zero growth**: no new SCC, member, merger, or incoming/internal
edge. Only then may that owner update an existing baseline:

```sh
ARCHITECTURE_BLESS=1 cargo nextest run -p workspace-architecture-check --run-ignored only
```

Before commit, inspect a set diff of old versus new package/SCC member and
`{from,to}` edge sets, not just textual/count deltas. Reductions are allowed;
new empty acyclic-package entries are distinct from graph growth. Require no
added cycle members or edges and an ordinary fresh unblessed gate passing on
the resulting baseline. A mixture of shrink and growth still fails; never
bless a regression or weaken the baseline. A verification-only assignment
reports the diff to its owner instead of modifying/blessing it.

## Launcher signing and return

For a separately commissioned joint run, build one launcher after the source
and prior writers settle:

```sh
cargo build --locked -p collaboration-central-tokio --features conformance --example central_sign_in_launcher
scripts/cargo_debug_signing_runner.sh --sign-only target/debug/examples/central_sign_in_launcher
codesign -v target/debug/examples/central_sign_in_launcher
codesign -dv --verbose=2 target/debug/examples/central_sign_in_launcher
```

Use the qualified outer route. Copy only the signed launcher to the private
artifact cohort, compare original/copied SHA256s and run both codesign checks
on the copy too. Require Developer ID Application, TeamIdentifier `974QD84WVC`
and Identifier `dev.shravansunder.test.central_sign_in_launcher`.
Publish actual source, five hashes, signature results, source/configuration
guards and the Miniflare/lock binding before the joint target is pinned.

Stop on any unsigned-execution path, Keychain/login prompt, failed signature,
unexpected tool replacement, changed source/metadata or failed command. No
unsigned fallback, global proxy/config repair or automatic retry is part of this
procedure. Send the exact failure to the Verifier; keep failed and corrected
evidence separate. Retain recorded owned PIDs, parent exits and reap/absence
readbacks, leaving all other processes and production state untouched.
