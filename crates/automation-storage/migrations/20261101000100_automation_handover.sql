-- Local handover custody exists before central owns the moved definition.
CREATE TABLE automation_handover (
    automation_id TEXT NOT NULL PRIMARY KEY,
    state TEXT NOT NULL,
    watermark_local_ms INTEGER NOT NULL,
    frozen_payload BLOB NOT NULL,
    digest TEXT NOT NULL
) STRICT;
