INSERT INTO registry_lease_incarnation(row_id, name, incarnation) VALUES (?, 'registry', ?) ON CONFLICT(name) DO UPDATE SET incarnation = excluded.incarnation;
