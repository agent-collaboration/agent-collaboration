# Vendor protocol tests

The ACP settings codec's colocated unit tests cover omitted and empty option
sets, grouped offers in provider order, and current select and Boolean values.
The runtime's scripted ACP integration tests exercise the catalog consumer.

Run the codec tests with `cargo nextest run --locked -p vendor-protocol`.
