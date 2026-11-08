-- How far the machine's Router has applied its notice stream (NoticesApplied).
-- NULL = no acknowledgment received yet. Domain state, not attempt state.
ALTER TABLE machine_control_project_progress ADD COLUMN applied_through INTEGER;
