-- Retain the exact central Bind after native preparation, before dispatch.
ALTER TABLE operation_receipts ADD COLUMN central_bind_request_json TEXT;
