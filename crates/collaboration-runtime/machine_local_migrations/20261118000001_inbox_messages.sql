-- The Router inbox owns delivery and Ack state. InteractionStore owns history.
CREATE TABLE inbox_messages (
 message_id TEXT PRIMARY KEY NOT NULL, -- Central's UUIDv7 MessageId, also the deduplication key.
 message_json TEXT, -- MessageRecord held until local handoff commits, then cleared.
 delivery_state_json TEXT NOT NULL, -- Rust-validated handoff or Ack state, including the stored answer.
 created_at INTEGER NOT NULL, -- First durable receipt, UTC Unix ms from the injected clock.
 updated_at INTEGER NOT NULL -- Latest local transition, UTC Unix ms from the injected clock.
);
