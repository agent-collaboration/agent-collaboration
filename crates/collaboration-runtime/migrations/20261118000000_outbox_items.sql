-- The Mac outbox (owner 2026-11-15): direct messages and thread posts wait here
-- for the next 1-minute flush; central's answer is kept for request_status.
CREATE TABLE outbox_items (
 request_id TEXT PRIMARY KEY NOT NULL, -- Caller-minted UUIDv7 RequestId; also the receipt id.
 caller_json TEXT NOT NULL, -- Caller the Router stamped; request_status is scoped to it.
 payload_json TEXT NOT NULL, -- RequestPayload as submitted; a resubmit is compared against it.
 item_state TEXT NOT NULL, -- Rust-validated: inOutbox | sent | accepted | refused; no SQL enum constraint.
 flush_at_ms INTEGER NOT NULL, -- The 1-minute mark promised on the receipt, UTC unix ms.
 outcome_json TEXT, -- Central's answer (ReplyPayload when accepted, RefusalReason when refused); null until answered.
 created_at INTEGER NOT NULL, -- UTC unix ms, from the injected clock.
 updated_at INTEGER NOT NULL -- UTC unix ms, from the injected clock.
);
CREATE INDEX outbox_items_by_state_and_mark ON outbox_items (item_state, flush_at_ms, request_id);
