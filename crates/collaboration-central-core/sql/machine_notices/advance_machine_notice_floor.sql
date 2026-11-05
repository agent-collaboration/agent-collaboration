UPDATE machine_control_progress SET pruned_through=CAST(? AS INTEGER),updated_at=CAST(? AS INTEGER) WHERE machine_id=?;
