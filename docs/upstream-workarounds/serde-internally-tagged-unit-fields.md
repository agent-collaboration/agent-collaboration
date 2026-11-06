# Closed internally tagged unit variants

Remote-create mail requires unknown fields to be refused on every variant.
Serde 1.0.228's derived decoder ignores extra fields on internally tagged unit
variants even when the enum has `deny_unknown_fields`. Its
`serde_derive/src/de/enum_internally.rs` selects `InternallyTaggedUnitVisitor`
for unit variants without consulting the container's unknown-field rule.

The remote-create contracts and `MailSender::Fleet` attach
`deserialize_closed_unit` to those variants. It decodes their payload through
an empty struct with `deny_unknown_fields`, preserving the public unit variant
and its existing `{"kind":"…"}` encoding.

The first contract run failed because a unit variant accepted `extra: true`:
`tmp/d2-s1/mail-green.log`. Reproduce through the permanent
`remote_create_contract_round_trips_every_closed_variant_and_refuses_unknown_fields`
test in `collaboration-mail-protocol`, removing the per-variant hooks. The test
checks the actual Serde decoder, every new variant and an unknown tag.

Remove the hooks and helper when the adopted upstream decoder enforces the
container rule for internally tagged unit variants and the unchanged contract
suite passes without them. No dependency fork or pin is carried.
