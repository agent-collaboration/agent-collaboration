UPDATE machine_control_project_progress SET expired_through=CAST(? AS INTEGER),updated_at=CAST(? AS INTEGER) WHERE machine_id=?;
