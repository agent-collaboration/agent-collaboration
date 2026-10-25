CREATE TABLE local_delivery_retries (
    item_id TEXT PRIMARY KEY NOT NULL,
    retry_attempts INTEGER NOT NULL
) STRICT;
