# UW-17 · Cursor reports a missing session as Invalid params

**In place.** On 2026-11-20, Cursor CLI `2026.11.13-eacade1` returned this ACP error to `session/load` after the Router closed the session locally. The session ID below stands in for the ID in the request.

```json
{"code":-32602,"message":"Invalid params","data":{"message":"Session \"<requested-id>\" not found"}}
```

ACP uses `-32002` (`Resource not found`) for an absent session. The Cursor adapter recognizes the reported `data.message` only for a restore request with code `-32602` and an exact match to the requested session ID in the quoted `Session "…" not found` clause. The Router classifies the result as `ProviderSessionNotFound`, tells the caller to start a new session with the same setup, and keeps the original provider code and diagnostic reference. [Cursor adapter](../../crates/acp-provider-adapters/src/cursor_behavior.rs) · [Restore mapping](../../crates/acp-client-runtime/src/provider_acp_error_mapping.rs).

To reproduce, use an isolated Cursor ACP session and send `session/load` for a previously recorded ID that the harness no longer has. Compare the returned error code and the quoted session ID with the request. The scripted Host test `cursor_missing_session_invalid_params_reaches_owner_as_lost_on_load_and_resume` replays the captured error shape through both restore methods. The adjacent tests cover a different ID and an unrelated invalid-parameters message. [Owner tests](../../crates/codex-router-host/src/external_provider_supervisor/tests/restore_recovery/cursor_lost_session_test.rs).

On Cursor upgrades, repeat the missing-session load and check resume if the version advertises it. Remove this workaround when Cursor returns `-32002` for the missing session and the Router still gives the lost-session answer using its standard mapping.
