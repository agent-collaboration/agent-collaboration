DELETE FROM machine_control_notices WHERE machine_id=? AND position<=CAST(? AS INTEGER);
