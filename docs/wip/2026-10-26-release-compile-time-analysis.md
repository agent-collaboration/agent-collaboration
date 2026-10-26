# Release compile time: where the 37 minutes go and how to cut them

Status: analysis handoff, 2026-10-26. Nothing in this document has been changed in code except the release-cache follow-up noted below.

## The question

The v0.1.103 release job (macOS, GitHub-hosted, 3 CPUs) took 37 minutes. That is under the 45-minute limit, but slow. What dominates it, and what is the cheapest real cut?

## What the release job does

From `.github/workflows/release.yml` at `b4881497` and the step timings of run 40395924377:

```text
tag v*  ─►  macOS job
            checkout, toolchain, metadata checks                       ~20 s
            restore cache  ── "No cache found"  (see Finding 1)
            prebuild optimized transport test + schema exporter        209 s   serial
            ├─ Build release binaries                                 1945 s   background
            ├─ Test optimized provider transport                         0 s   background
            ├─ Export release protocol schemas                           1 s   background
            └─ Reject test-only production features                      6 s   background
            wait-all                                                  1944 s
            sign, verify, package, upload                               ~30 s
        ─►  Homebrew tap job                                            ~60 s
```

The background steps work as intended. They overlap only about 7 seconds of work with the build, though, so they are not where the time goes. The job is one long `cargo build --release`.

## Finding 1: the release cache can never be saved

`release.yml` restores `Swatinem/rust-cache` with `shared-key: release-arm64-background` and `save-if: push to refs/heads/main`. The workflow triggers only on `v*.*.*` tags and `workflow_dispatch`, so the save condition is never true and every release starts cold.

Follow-up in flight: branch `fix/release-cache-warm` adds a main-only macOS job that builds the same release binaries and saves that cache key, so tag runs can restore it. GitHub documents that a run can restore caches from the default branch; the first tag after it merges is the proof.

Expected gain is about 12 minutes (Finding 2). Cache budget: main's caches are 6.15 GB today; the release cache adds about 2 GB, under the 10 GB free limit. Caches must never be saved from pull requests. PR #169's old-workflow branch saved 4.5 GB from a PR and pushed the repo over 10 GB; those caches were deleted.

## Finding 2: our own crates are most of the build, and caching cannot help them

Cargo's `--timings` report from the same run (artifact `release-timing-inventory`): 915 units, 88 CPU-minutes, 32.4 minutes of wall time on 3 CPUs.

- Third-party dependencies occupy roughly minutes 0 to 10. `Swatinem/rust-cache` caches only these.
- Our workspace crates account for 61 of the 88 CPU-minutes. They change in every release, so no cache saves them.

The critical path is a chain of our crates:

| Crate | Compile (release) | Wall-clock window |
|---|---|---|
| `collaboration-service` | 9.0 min | 10.7 → 19.7 |
| `collaboration-mcp` | 12.2 min | 11.7 → 23.9 |
| `codex-router-host` | 10.8 min | 13.4 → 24.2 |
| `agent-collaboration` | 6.9 min | 20.4 → 27.3 |
| `codex-router-cli` (lib) | 4.5 min | 24.3 → 28.8 |
| `codex-router` binary (link) | 3.6 min | 28.8 → 32.4 |

With a warm dependency cache, the expected release time is about 25 minutes. That is the floor for the current crate structure on a 3-CPU runner.

## Finding 3: crate sizes

Production lines at `origin/main` (`b4881497`), excluding in-crate tests:

| Crate | Production lines | Files | In-crate test lines |
|---|---|---|---|
| `collaboration-service` | about 50k (module graph count) | 253 | about 47.5k |
| `agent-collaboration` | 33.7k | 151 | 21.1k |
| `codex-router-cli` | 28.4k | 101 | 35.6k |
| `codex-router-host` | 26.8k | 107 | 28.1k |
| `collaboration-mcp` | **4.4k** | 25 | 9.9k |

No production file in `collaboration-service` is over 1,000 lines. The owner's rule is no file over 1,000 lines anywhere, with lock files excepted.

`collaboration-mcp` is the outlier: 12 minutes for 4,390 lines means the cost is generated code, not size. The candidates are the 39 rmcp `#[tool]` handlers under `#[tool_router]`, the schemars `JsonSchema` derives, and large async state machines. It isn't caused by generic code from `collaboration-service`, because that crate's `CollaborationApplication` is not generic. See Finding 5 for the measurement.

## Finding 4: where `collaboration-service` can be split mechanically

The module graph comes from `crate::<module>` references between its 117 top-level modules, grouped by domain prefix. Production code only.

```text
ABOVE the knot (depends on it; nothing inside the crate depends on these)
  project_admission (5.6k) ─► notification_publisher (2.2k)
  session (1.2k)
                    │
                    ▼
THE KNOT: one dependency cycle, about 30k lines
  collaboration_application (11.7k) ⇄ provider (3.5k) ⇄ native (2.0k)
  ⇄ schedule / run / summary_native_worker (3.8k) ⇄ codex (2.3k) ⇄ board (0.9k)
  ⇄ automation (0.8k) ⇄ mail_delivery_runtime / mail_push_outcome /
    push_record_delivery / service_identity (about 4.6k) ⇄ operation_receipt_projection
                    │
                    ▼
BELOW the knot (no dependency on it; leaves)
  interaction (2.5k)   delivery + wake (1.3k, a two-module cycle)
  app_server + router_session_app_server (2.1k)   fleet (0.9k)   message (0.4k)
  project_store_registry / project_store_paths / central (about 0.6k)
```

- **Mechanical, with no cycles to break:**
  - A crate below `collaboration-service` with the leaf groups, about 8k lines.
  - A crate above it with `project_admission`, `notification_publisher` and `session`, about 9k lines.
- **Not mechanical:** the knot. `collaboration_application` calls provider, native, schedule, codex and mail delivery, and they call back into its types. Splitting it means inverting some of those edges behind interfaces. That is design work and an owner decision.
### Why `collaboration_application` has no DAG or injection today

`CollaborationApplication` holds only a `ServiceIdentity` (`collaboration_application/mod.rs:66-74`). Each operation family is built on demand with concrete collaborators, for example `WakeOperations::new(&service, Some(&store))`. Nothing is injected behind an interface, and Rust allows cycles between modules inside one crate, so nothing forced layering.

The 30k-line cycle comes from only **16 production back-references** from lower modules into `collaboration_application`:
- **Shared types placed in the application module** (most of them): `NativeSessionFailure*`, `ProviderInventoryFailure*`, `ScheduleOperationFailure`, `CollaborationRejection`, `MessageFailureKind`, `WakeWaitFailure`, `ResultByteBudget` and `INVALID_INVENTORY_PARAMETERS`. Users include:
  - `codex_session_inventory.rs:3`
  - `provider_session_inventory.rs:4`
  - `schedule_preparation.rs:8`
  - `push_record_delivery.rs:31,273,287`
  - `board_views/card_projection.rs:122`
- **One real upward call:** `mail_delivery_runtime/remote_wakes.rs:290` constructs `WakeOperations` (an application operation) inside the mail runtime, plus wake failure helpers at `:399` and `:417`.
- `native_control_backend.rs:6` references the application only under `#[cfg(test)]`.

**Measured:** with those back-references removed, the module graph is a DAG apart from three small local cycles:
- delivery ⇄ wake (1.3k lines);
- schedule ⇄ run ⇄ summary_native_worker (3.8k);
- mail_delivery_runtime ⇄ mail_push_outcome ⇄ service_identity (4.4k).

Moving the shared failure and budget types to a module (or crate) below the application, and inverting the one remote-wake call behind a small trait that the application implements and injects into the mail runtime, is therefore close to mechanical. It turns the knot into layers that can become separate crates.

- **Expected effect:** modest. About 16k of 50k lines leave the 9-minute crate, maybe 9 → 6 minutes on that link, and the crate above still sits in the chain. The bigger single item is `collaboration-mcp` (Finding 5).

How to reproduce the graph: list `mod` declarations in `crates/collaboration-service/src/lib.rs`, collect `crate::<module>` references per module's non-test files, group by prefix, and compute strongly connected components.

## Finding 5: what generates the code in the slow crates

Measured with `cargo llvm-lines --release -p <crate> --lib` (cargo-llvm-lines 0.4.48) at `b4881497`. This counts the LLVM IR each function generates and how many monomorphized copies it gets.

| Crate | LLVM IR lines | Function copies |
|---|---|---|
| `collaboration-mcp` | 6,943,170 | 144,784 |
| `codex-router-host` | 6,425,358 | 133,301 |

`collaboration-mcp`'s IR, attributed to the crate each function body comes from:

| Origin | Share | IR lines | Copies |
|---|---|---|---|
| `core` + `alloc` | 19.3% | 1.34M | 56k |
| `serde` + `serde_json` | 19.6% | 1.36M | 28.6k |
| `collaboration_service` | 8.3% | 574k | 3,485 |
| `message_board` + `message_board_storage` | 12.4% | 863k | 12.4k |
| `rmcp` | 6.4% | 446k | 4,456 |
| `collaboration_protocol` | 6.1% | 421k | 6,971 |
| `tokio` + `sqlx_core` | 7.1% | 493k | 10.7k |
| **`collaboration_mcp` itself** | **3.0%** | 208k | 1,920 |

**Conclusion:** 97% of the slowest crate's code generation is generic code from other crates, instantiated and optimized again inside `collaboration-mcp`. Examples:
- `collaboration_service` internals: `summary_native_worker::step`, `ViewSnapshot::load`, and three copies of `BoardOperations::project_feed_page::<…>`;
- `message_board::project_requests::BoardOperation`'s serde `Deserialize`, in 42 copies through serde's `ContentDeserializer`, the buffering path that internally tagged and untagged enums use;
- the sqlx pool and `fleet_sync_writer::ReplicatedDatabase`.

Stable Rust shares generic instantiations across crates only in debug builds, not release. So each downstream crate pays for its own copies, and `codex-router-host` shows the same total (6.4M lines). Splitting `collaboration-service` alone would not shrink this. The cost moves to wherever the generic code is instantiated.

Levers, to be measured one at a time with the same tool:
- **Monomorphize once in the owning crate.** Give hot generic entry points concrete, non-generic wrappers (or `dyn` at the crate boundary) in the crate that owns them, so downstream crates call compiled code instead of instantiating it. Candidates: `BoardOperations::*_page::<T>`, the service operations the 39 MCP tools call, and the storage and pool paths.
- **Deserialize `BoardOperation` once.** Expose one non-generic `fn` in `message_board` that parses it from a `serde_json::Value` or bytes. If the 42 copies come from an internally tagged enum, consider an adjacently tagged or hand-dispatched form after measuring.
- **Check the rmcp tool router.** Per-tool wrappers, `JsonSchema` derives (for example `ConversationCreatePromptToolRequest`'s schema at 4.4k lines) and two `serve_inner` instantiations, one per transport.

Raw outputs, local only (not committed): `tmp/compile-time/*.txt` in the analysis worktree.

## Options, cheapest first

1. **Merge the release cache** (`fix/release-cache-warm`): about 37 → 25 minutes. No code changes.
2. **Cut cross-crate monomorphization** (Finding 5): non-generic entry points in the owning crates, and one-place deserialization of `BoardOperation`. This is the largest lever on the two slowest crates (`collaboration-mcp`, `codex-router-host`). Measure each change with `cargo llvm-lines` and Cargo `--timings`.
3. **Mechanical `collaboration-service` split** (Finding 4): modest gain, low risk. It does not address Finding 5 by itself.
4. **Untangle the knot**: design work, with the largest structural payoff, also for CI test builds.
5. **Larger runner**: ruled out (costs money; free-tier rule).

## Pointers

- Release run: 40395924377. Its timings artifact is `release-timing-inventory`, with three `cargo-timing-*.html` files.
- Analysis worktree: `codex-router.analysis-compile-time` (branch `analysis/compile-time`).
