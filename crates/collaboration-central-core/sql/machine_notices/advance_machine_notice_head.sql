UPDATE machine_control_project_progress SET last_issued_position=CAST(? AS INTEGER),updated_at=CAST(? AS INTEGER) WHERE machine_id=?;
