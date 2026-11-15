# Issuer-owned machine-login mirrors

These files are copied byte for byte from Switchboard commit
`b1c9f6f616f5f77fbc87eaec877120ac06f4b1d3`, under
`packages/sign-in-core/contract-fixtures/machine-login/`.

- `machine-login-validation-vectors.json` mirrors `validation-vectors.json`: 106 vectors, including three explicit refusals for removed former-service inputs.
- `machine-login-decision-variants.json` mirrors `decision-variants.json`: five device-login and six machine-token variants.

The integration consumer pins each source hash and applies every issuer verdict
and accepted output without local exceptions. Changes come from the issuer;
central does not edit or regenerate these JSON files.
