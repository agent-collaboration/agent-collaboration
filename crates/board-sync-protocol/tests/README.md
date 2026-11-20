# board-sync-protocol tests

Run the package with its workspace CI features:

```sh
cargo nextest run --locked --profile ci -p board-sync-protocol
```

`integration/main.rs` checks the frozen local SDK mail contract. It covers
message, notification and wake body encoding and wire budgets for addresses,
senders, routes and acceptance evidence. Library unit tests stay colocated with
the delivery types.
