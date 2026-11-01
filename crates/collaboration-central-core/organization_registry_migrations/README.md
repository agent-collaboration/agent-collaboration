# Organization Registry schema

The owner approved the complete first migration on 1 November 2026 through Lead message `01a196eb-8977-71c1-a502-dbdc5e40511c`, SHA256 `6835cfebea23463f69501e30a36bdc02557019dfdfc3ac33e41024158f659167`. Its bytes, including the original proposal comments, are preserved exactly. It defines two business tables and the immutable creation-audit table. Both hosts use this one core-owned corpus.

The durable-steps journal retains creation retry state. Rust validates domain values and enforces authorization/organization/claim guards at storage-transaction boundaries. No table in the existing lease Registry, authority or Fleet corpus is changed here.

Central has not registered or executed this corpus. Machine-claim metadata currently describes it only in the query generator's scratch database; the organization foreign-key target awaits the ownership ruling and must be regenerated if the approved schema changes.
