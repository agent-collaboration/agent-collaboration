# Organization link seam: keeping new Fleet call modules out of core's cycle

`collaboration-central-core` has one large module cycle. The module-cycle
ratchet (`architecture/module-cycle-baseline.json`) refuses any new edge
inside or entering it. Older Fleet call modules (`fleet_metadata_calls`,
`fleet_read_calls`, …) entered that cycle the usual way: they `impl` handlers on
`ObjectRuntime` and use the call envelope, caller context, step driver and
`PlatformIo` directly. A new call module written that way fails the ratchet.

## The shape

Split each call module into two halves.

**The decision module** (for example `organization_provisioning_calls`):
- holds the `CallTarget` types and their answers;
- holds free functions that make the decisions, taking a typed request and
  `&impl <NarrowTrait>`;
- defines the narrow trait (here `OrganizationLinkFleet`), named for what the
  handlers need from the Fleet object: its organization, its stamp authority,
  clock and IDs, observation, and one method per store step;
- depends on no module in core's cycle.

**The runtime adapter**, a child module of `fleet_runtime` (here
`fleet_runtime/organization_link_fleet.rs`). cargo-modules folds child modules
into their top-level module, so it adds no new top-level edges. The adapter:
- checks the envelope (face and authority) and decodes target and request;
- borrows the Fleet's parts into a view struct that implements the narrow trait;
- runs each store step through `StepDriver` (the `StepCallback` structs live
  here);
- encodes the answer.

The only edge between the halves runs from the runtime to the decision module,
which leaves the cycle; nothing enters it.

## Rules carried over unchanged

- Clocks the applying transaction needs are passed in as `Arc<dyn Clock>` and
  sampled inside the transaction, as before.
- Each store step is still one transaction; the adapter adds no transactions
  and removes none.
- Refusals, answers and observations are produced by the decision functions,
  in the same order as before.
- Conformance hooks (`before_apply`, `take_record_fault`) are trait methods
  behind `feature = "conformance"`.

## Status

Piloted on `organization_provisioning_calls`. The other three B2 modules
(`organization_admin_calls`, `organization_link`, `organization_admin_link`)
follow once the pilot is reviewed.
