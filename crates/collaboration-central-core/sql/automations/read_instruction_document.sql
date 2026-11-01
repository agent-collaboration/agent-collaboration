SELECT instruction_id,current_content_id,CAST(created_at_ms AS TEXT) AS created_at_ms,CAST(updated_at_ms AS TEXT) AS updated_at_ms FROM instruction_documents WHERE instruction_id=?;
