# Collaboration client tests

Run the package with nextest:

```sh
cargo nextest run --locked --profile ci -p collaboration-client
```

`integration/main.rs` gathers the 19 former external roots. Cases cover typed
API requests, refusals and lost responses; ACP conversations and native adapter
settlement; service and repository discovery; subscriptions, permissions and
nonregular history files. Library unit tests remain colocated.

The aggregate declares the shared scripted API fixture once. Two former roots
register the same two ACP cases: their four existing case names call one copy
of each execution body in the colocated `acp_aggregate_prompt_outputs_test.rs`.
Case expansions and fixture data retain their existing directories. CI selects
no separate e2e category, so no empty e2e target is declared.

A former binary `<root>` and test `<case>` becomes binary `integration` and
test `<root>::<case>`. Library test identities do not change. Nextest preserves
one process per case and the existing scheduling and deadline rules.

The FIFO child selector derives from `module_path!()`. Its parent captures
owned stdout and requires one passed child test, retaining the original
five-second deadline and kill/reap behavior. A zero-match child cannot pass.
