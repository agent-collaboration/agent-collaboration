# Adopted S6 fixture outcome readback for U4 and U5

**Adopted for the Lead's S6 brief** by push
`01a18ce0-eea3-7eb3-b6bb-18b1df4f05d5`; no producer or product change is made here.
The Implementer owns the producer, feature-gated test support only and absent
from normal/release builds. This remains unobserved until its real bytes reach
the real reader with the same-ID controls below.
The current terminal marker prints only `selection.session_id()`. That discards
which `SessionPickerIdentity` variant and endpoint owns the opaque ID. U4 now uses same-ID HostedCodex fixture rows across different services and
endpoints (Lead push 01a18e4a-bb03-7553-bb56-b784c5ad45e3). The earlier Gamma/Echo
HostedProvider return-path proposal is superseded: provider Open stays inside
the picker client-action route and never returns ResumeSession. Its captured
provider target/effects stay assigned to the existing client-action component
tests; those tests are not rerun or claimed green by this reader checkpoint.

## Smallest readback

Have the existing test-only entry
`crates/agent-session-tui/src/session_picker/picker_entrypoint.rs::run_sessions_picker_test_harness`
write one durable `picker-outcome.json` in the already owned harness receipt
location, **after** the real render loop returns. Emit from the returned
`SessionsPickerOutcome`, never from the current browse row or label. Keep the
human marker derived from that same outcome; the U4 checker never falls back to it.

The fixture-local DTO has one kind tag and the actual typed identity. Later
Lead pushes 01a18e33-0249-7fa0-9756-eeebb1a22d1c and
01a18e37-0d51-78d2-b22c-01996654eb1d add the returned New request; see the
[current reader recipe](input-consumption-readback.md).

```ts
type PickerOutcomeReadback =
  | { kind: 'new'; request: { client: 'codex' | 'claudeCode'; machine: string; directory: string; runMode: 'foreground' | 'backgroundThenAttach' } }
  | { kind: 'cancelled' }
  | { kind: 'resume'; identity: PickerIdentityReadback }
  | { kind: 'fork'; identity: PickerIdentityReadback };

type PickerIdentityReadback =
  | { kind: 'localCodex'; sessionId: string }
  | { kind: 'hostedCodex'; target: SessionRef }
  | { kind: 'hostedProvider'; target: SessionRef }
  | { kind: 'nativeClaude'; target: SessionRef };
```

`SessionRef` is the existing serialized protocol type, not a second handwritten
schema: `{endpoint:{serviceId,endpointId},sessionId}`. Its service identity is a
UUID; endpoint and session IDs are their existing validated opaque types. Reuse
its Serde implementation. Session IDs are nonempty, not necessarily UUIDs.

The producer DTO belongs adjacent to the feature-gated entry. Match the existing
`SessionPickerIdentity::{LocalCodex,HostedCodex,HostedProvider,NativeClaude}`
exhaustively, copying from `SessionActionSelection.identity`. Map ResumeSession
and ForkSession to their corresponding tags; `None` is cancelled. Other picker
outcome variants must be explicitly reported as outside this U4 fixture case,
never coerced into Resume/Fork or a fabricated target.

The reader's home is `scripts/verification/tui/sessions/source-qualified-outcome.ts`,
with a strict schema and whole-value comparison to an independently declared
fixture target. The implementation now exists against the adopted wire; real S6b byte
binding remains pending. A wrong kind, missing target, unknown field,
different service/endpoint/identity variant or same-UUID foreign target rejects.

No title, display machine, provider label, model, credential reference, address or
full RouterConnectionProfile is needed for this U4 identity claim. Configured
machine dispatch policy and native/provider job execution remain separate F
proof. If S6 also needs route-context proof, add only the validated connection
name as a separately agreed projection of `PickerSourceContext`; never serialize
the full profile to get it accidentally.

## Proof the reader must earn

- Select Alpha, dispatch Resume, require LocalCodex plus its exact opaque ID.
- Select same-ID HostedCodex rows from independently declared different
  services and endpoints, require HostedCodex and the full exact target for both
  Resume and Fork. A wrong service, endpoint or identity family with that same ID
  must fail; HostedProvider is a wrong-family control, not a return-path fixture.
- Retain wrong-service, wrong-endpoint, wrong-kind, missing-target and extra-field
  controls. Also check Fork separately and ordinary Enter → Details → Cancelled.
- The real producer's serialized DTO must pass the real reader. A hand-built JSON
  fixture qualifies only reader sensitivity, not picker wiring or effect identity.
- Keep source/build/byte bindings outside this DTO in the existing capture receipt.
  Local/default New uses the single returned new outcome and exact captured
  request as U5 proof. Only configured-source New retains the separate counted
  preparation stand-in; do not require that counter on local/default paths.

Source anchors: `agent-session-inventory/src/session_commands/session_action_selection.rs`
(`identity` is frozen from the captured record; `session_id()` drops qualification),
`session_catalog_records.rs::SessionPickerIdentity`, and
`collaboration-protocol/src/endpoint_identity.rs::SessionRef`.
