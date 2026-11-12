# Private Tailnet exposure lifecycle

**Draft: real-target journeys remain unobserved.** Use only after the caller declares the current normal signed artifacts and a live-run allocation. Loading this recipe grants no Host, SSH, Serve, profile, native/model or fault action. A frozen source tree, fixture tests, ping or a compiler lease is not that declaration.

The outcome comes from [Tailnet Specification R1–R7](../../../../docs/specs/2026-10-16-tailnet-collaboration/specification.md), its [Requirements](../../../../docs/specs/2026-10-16-tailnet-collaboration/requirements.md) and [HTTP Program Design](../../../../docs/specs/2026-10-16-tailnet-collaboration/program-design.md). The board journey that reached a peer Host’s board store retired with the board in the workspace storage clean-up; this recipe keeps the Host exposure lifecycle. The Host remains unprivileged; existing Origin checks remain enforced. Each Host owns its foreground Serve child and session, while production and unrelated configuration remain protected.

## Entry and required inputs

Run from the repository root. The [Rust Host driver](../../../../crates/tailnet-verification-host/README.md) owns the Host lifetime, profile guard and process cleanup. One agent owns each instance for its whole run. The caller sequences the two machines and any separately authorized transport.

Build the Host with `cargo build -p codex-router-tailnet-verification-host`. Bind source and executable hashes on both machines; the Host records them in `driver-binding.json`.


Have the caller supply actual values for this plan before any live command:

- **HostPlan on each machine:** source repository/head, immutable normal three-binary qualification and its SHA256, expected version, a nonexistent direct `/private/tmp` root, a freshly checked free proxy port, private node HTTPS `/mcp` URL and HTTPS port, the installed Tailscale executable, original debug-profile SHA256, protected process identities and an aware future deadline. The qualification shape is `sourceHead` plus `artifacts` containing `name`, absolute `path`, `sha256`, `features` and `version`; the driver rechecks bytes, normal features, actual versions and strict debug signing. The caller binds the actual source/diff to those artifacts; matching HEAD alone does not establish a binary’s source.

`HOST_PLAN` below denotes a caller-supplied absolute JSON plan path, not an example payload. The two Host roots and local evidence paths belong to the declared allocation. Preserve real owner HOME and normal Codex home. The driver checks those homes, disables unrelated external providers only in its fresh root and permits only a hash-bound temporary port projection in the normal debug profile. It does not copy credentials or account data. Stop at a permission, ownership, existing-root, profile-hash or selected-port conflict; do not change identity, use sudo, alter Tailscale policy or substitute a different backend to get past it.

## Start and establish readiness

On each machine, within its live grant:

```sh
target/debug/codex-router-tailnet-verification-host host "$HOST_PLAN"
```

This invokes the declared normal `codex-router host` with the fresh root, selected proxy port, `--mcp-bind 127.0.0.1:0`, `--require-debug-isolation`, `--remote-access tailscale` and selected HTTPS port. It keeps that instance alive until its explicit owned stop request or deadline. Deadline expiry is failure, not a pass or authorization to retry.

The initial Host-readiness observer allows at most 150 seconds, capped by the caller's remaining allocation: two five-second preflight commands, 20 seconds for foreground advertisement, five seconds for post-start status, the adopted 75-second product local-MCP health budget and 40 seconds for native publication/observation. See [server health and peer TLS](../../../../docs/specs/2026-10-16-tailnet-collaboration/program-design.md). The cap is computed once; it is not reset for pending inventory. Caller-deadline, process/identity/failure/cleanup checks and the no-write-replay rule remain enforced. This is an observer allowance, not evidence that a certificate or peer became ready; the live allocation is never enlarged.

Require this allocation’s `evidence/host-ready.json`, bound to its live `host-started.json` identity, `driver-binding.json` and declared artifact. A responding port or old file is insufficient. Readiness checks:

1. The published version3 manifest has the expected service version and proxy endpoint. Its MCP selector must contain exactly `transport: streamableHttp` and a valid loopback HTTP `/mcp` URL; wrong/missing transport or unknown selector fields fail.
2. Actual local Unix MCP inventory and inventory from the manifest's nonzero bound loopback MCP URL name the same intended service ID/epoch. The loopback inventory request uses the exact validated proxy Host authority and no Origin. The native endpoint is available with a matching generation; this is local endpoint readiness, **not** peer TLS or native session-action proof. `host-ready.json` retains both inventories and labels peer TLS unobserved. No self-HTTPS request is a startup prerequisite.
3. Before/during Serve state preserves unrelated entries and shows exactly one owned private foreground HTTPS session whose `/` handler forwards to the actual bound loopback MCP address. A Unix proxy, different backend, collision or target Funnel fails. The session ID is recorded.
4. Read-only `tools/list` probes on that actual loopback listener accept native Host/no Origin, the existing loopback Origin, exact proxy Host/no Origin and existing loopback Origin with the proxy Host. Wrong node/port and both HTTP/HTTPS proxy Origins return 403. At HTTPS 443 only, the exact bare node Host is also accepted as 443; non443 requires its explicit selected port. Origin is never rewritten.

The manifest is published before the lifecycle owner calls `backend_ready`. Within that bounded readiness loop, only the validated same-invocation inventory containing both explicitly disabled provider entries and no native entry remains pending. The first manifest pins the service ID/epoch; a changed invocation, malformed inventory, different disabled-state reason, or a present native entry lacking valid availability/generation fails immediately. This waits for the source-backed initial publication state without catching arbitrary errors, retrying mutations or resetting either deadline.

These inventory/probe requests are the read-only health route. After any failed drive, preserve its raw response and bindings, and check the same intended service/epoch through that route before any further separately authorized drive. An unexplained failure is not permission to relaunch, retry a mutation or proceed on another target.

The adopted path is CLI Host configuration → `HostCollaborationInputs` → prepared Tailscale preflight → bound loopback API with the exact proxy authority → foreground Serve → local service/epoch health. Decisive owners are [CLI launch](../../../../crates/codex-router-cli/src/host_command/foreground_launch.rs), [runtime composition](../../../../crates/codex-router-host/src/collaboration_runtime.rs), [exposure](../../../../crates/codex-router-host/src/tailscale_exposure.rs), [API listeners](../../../../crates/codex-router-host/src/collaboration_runtime/collaboration_api_serving.rs) and [Host normalization at port 443](../../../../crates/collaboration-mcp/src/proxy_authority.rs). Local health/configured Serve does not establish an allowed peer's ACL, route, trust or certificate-validated access; actual peer HTTPS remains a separate real-target gate.

## Normal shutdown and restoration — R3, R4

After all callers settle, request stop only through that owned Host’s `evidence/stop.json` within the caller’s allocation. Preserve the evidence directory. The lifecycle records stable acquired identities and `host-before-fallback.json` before driver cleanup signals, requests the exact Host to stop, then uses the canonical identity-guarded fallback cleanup if necessary.

For a normal-shutdown pass require:

- `host-settlement.json`: exit 0, no fallback `signals`, no cleanup `errors`, empty `remaining`;
- before/during/after Serve evidence: exact owned foreground session retired and unrelated configuration unchanged; no reset or stale full-state replay;
- `lifecycle-settlement.json`: null failure, empty cleanup errors, original debug profile bytes restored exactly, and unchanged protected process identities.

The pre-cleanup snapshot is tagged `observed` with retained `recorded` identities and currently matching survivors, or `unavailable` with an error. Missing/denied/inert observation never counts as empty. Evidence-write failure still attempts owned cleanup and prevents a passing apparatus result. A concurrent profile change is left untouched and reported; never blindly restore over it. No directory removal is part of this recipe.

## Separately declared after-ready exposure-loss negative — R4

This is a separate caller-declared scenario, not an automatic step of `host`. Before any signal, the caller must record its permitted fault, expected nonzero Host outcome, complete pre-fault ownership inventory and exact foreground Serve child/session/backend binding. Revalidate the selected child’s PID/UID/birth/image/group immediately before signaling that child once. Never target tailscaled, a process name/group, a foreign child or the Host itself. This recipe grants no signal.

Retain the child’s actual settlement, raw Host error naming exposure failure and the Host’s own bounded nonzero exit. A stop request, allocation deadline, observation error or apparatus-issued Host termination is not that causal chain. The normal driver correctly records unsolicited Host exit as a failed positive run; preserve that failure. It may also supply evidence for the prior negative intent, without relabeling the positive run as passed.

To attribute cleanup to the Host, bind the pre-fault inventory to `host-before-fallback.json`, require `kind: observed` with no matching survivors, and inspect Host settlement for zero fallback signals/errors/remaining. Otherwise cleanup is assisted or unknown. Retain exact Serve retirement, profile restoration and protected-identity results separately. The product path is typed `TailnetExposure` failure → owning collaboration shutdown → existing Host settlement → explicit Host error; see [Host event loop](../../../../crates/codex-router-host/src/lifecycle_owner.rs) and [settlement](../../../../crates/codex-router-host/src/lifecycle_owner/lifecycle_convergence.rs). One other declared Host may supply the separately scoped normal-shutdown proof; check common-build applicability when combining the results.

## Apparatus checks and proof limits

The permanent local suite is:

```sh
cargo test -p codex-router-tailnet-verification-host
cargo clippy -p codex-router-tailnet-verification-host --all-targets -- -D warnings
```

These tests use literal protocol fixtures and harmless retained processes. They qualify the apparatus only. Preserve failed/unknown IDs and raw results; do not weaken an expected outcome to manufacture a pass.


**Open coverage:** final normal target and live-run lease; actual peer HTTPS/Origin access, normal retirement and the separately declared R4 negative. Default-mode no-IO/no-alias, parser/replacement and full Host-supervisor product tests remain separate required gates. Fixtures, source inspection and this draft cannot close them. Return each real journey as supported, unsupported or unknown with machine/build/root/client, exact evidence paths and cleanup outcome. Acceptance remains with the caller.
