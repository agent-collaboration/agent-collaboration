SELECT batch_id,service_id,frozen_bytes,digest FROM automation_batches WHERE service_id=? AND batch_id>? AND acked_at_ms IS NULL ORDER BY batch_id;
