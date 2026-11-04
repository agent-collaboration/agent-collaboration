# Message board tests

Run the package with nextest:

```sh
cargo nextest run --locked --profile ci -p message-board
```

`integration/main.rs` gathers the 32 former external roots. Cases cover record
and domain contracts, participants and roles, project and thread lifecycles,
receipts and refusals, subscription membership, cursors and work rules. Library
unit tests remain colocated with their implementations.

The aggregate declares shared record-envelope and actor-origin fixtures once.
Case expansions and fixture data retain their existing directories. No empty
e2e target is declared because CI selects no separate e2e category here.

A former binary `<root>` and test `<case>` becomes binary `integration` and
test `<root>::<case>`. Library test identities do not change. Nextest preserves
one process per case and the existing scheduling and deadline rules.
