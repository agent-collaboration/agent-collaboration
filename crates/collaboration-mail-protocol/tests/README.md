# collaboration-mail-protocol tests

This crate provides the shared `SessionDisplayName` type and schema. The
display-name validation cases live in `collaboration-protocol`:

```sh
cargo nextest run -p collaboration-protocol -E 'test(display_names_)'
```
