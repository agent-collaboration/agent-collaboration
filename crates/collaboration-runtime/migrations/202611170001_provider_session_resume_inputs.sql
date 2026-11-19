-- What a resumed session needs to rebuild the setup it was created with
-- (D1 R18; owner Decision 3, option A): its requested mode, the work roots
-- its creator named, and its scratch folder. Additive and nullable, with no
-- CHECK constraints (validated in Rust). Rows written before this read NULL,
-- and a resume keeps the fallback: no Router allowance, every request asks.
-- The table keeps its composite key from before the UUIDv7-key rule; this
-- change adds columns only.
ALTER TABLE provider_session_records ADD COLUMN requested_mode TEXT;
ALTER TABLE provider_session_records ADD COLUMN work_roots_json TEXT;
ALTER TABLE provider_session_records ADD COLUMN scratch_folder TEXT;
