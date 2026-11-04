ALTER TABLE fleet_project_directory ADD COLUMN copy_epoch TEXT;
ALTER TABLE fleet_project_directory ADD COLUMN copy_sequence TEXT;
ALTER TABLE fleet_project_directory ADD COLUMN copy_acked_at INTEGER;
ALTER TABLE fleet_project_directory ADD COLUMN copy_latest_time_basis TEXT;
ALTER TABLE fleet_project_directory ADD COLUMN copy_latest_record_at TEXT;
