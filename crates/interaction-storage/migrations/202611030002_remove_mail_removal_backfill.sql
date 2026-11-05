-- Preserve migration lineage for stores opened at the preceding D1 pin.
-- The library's _fleet_sync_delivery_backfills now owns both startup passes.
DROP TABLE mail_removal_backfill;
