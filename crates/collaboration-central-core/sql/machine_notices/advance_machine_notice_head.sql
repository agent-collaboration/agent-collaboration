UPDATE machine_control_progress SET last_position=CAST(? AS INTEGER),updated_at=CAST(? AS INTEGER) WHERE machine_id=?;
