SELECT state.state_id, state.name,
       CAST(state.last_position AS TEXT) AS last_position,
       typeof(state.last_position) AS position_class,
       CAST(state.retained_through AS TEXT) AS retained_through,
       typeof(state.retained_through) AS floor_class,
       CAST(COALESCE(state.has_state, 0) AS TEXT) AS state_exists,
       CAST(EXISTS(SELECT 1 FROM fleet_feed) AS TEXT) AS feed_exists,
       CAST(EXISTS(SELECT 1 FROM automations)
         OR EXISTS(SELECT 1 FROM automation_timing)
         OR EXISTS(SELECT 1 FROM automation_firings)
         OR EXISTS(SELECT 1 FROM automation_batches)
         OR EXISTS(SELECT 1 FROM machine_work_items) AS TEXT) AS automation_work_exists
FROM (SELECT 1) AS alarm_root
LEFT JOIN (
    SELECT 1 AS has_state, state_id, name, last_position, retained_through
    FROM fleet_feed_state
) AS state ON 1 = 1;
