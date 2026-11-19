# fleet-engine tests

The colocated unit tests exercise the reducers, compare-and-set attempt
statements, receipts, and query registry. Run them with
`cargo nextest run --locked -p fleet-engine --features test-support`.
The Tokio and Worker integration suites retain the real-host cases.
