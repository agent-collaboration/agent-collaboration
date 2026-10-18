# M2.6 legacy upgrade proof holding area

A Lead disposition binds these eight original claims to the held one-time offline upgrade (U54). The single cfg gate in ../legacy_upgrade_m2_6.rs keeps them out of the new Router build until that script exists. No assertion was removed.

- schema_migrations.rs: four old schema/checkpoint adoption and data-retention claims.
- subscription_migration.rs: three old subscription schema/backfill claims.
- history_text.rs: historical stored control-character escape behavior; its arrangement explicitly predates the write boundary. Current PostText rejects controls, so the old-history treatment belongs to translation.

The fixture/helper copies retain their original contracts solely as held test inputs. They provide no production store, alias or adapter. Ordinary behavioral claims migrate to current public admission/native fixtures in M2.5b. Restore all eight when the upgrade script is designed; the whole workspace test build must be green before the M2 PR.
