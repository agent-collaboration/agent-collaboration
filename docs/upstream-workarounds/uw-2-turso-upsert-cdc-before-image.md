# UW-2: Turso conflict updates capture the wrong before-image

**Status:** in place, 2026-10-13
**Lives in:** `message-board-storage` project record relations, watches and participant intervals
**Design:** project storage program design, “No upserts on capturing connections”

## Problem

Turso 0.8.1 builds a conflict update's CDC before-image from the table cursor after replacing the row. A length-changing update can fail decoding that image; another update can succeed while capturing the new value or a neighboring row as its before-image. The project feed and the captured changes must both remain correct.

Upstream issue [#9069](https://github.com/tursodatabase/turso/issues/9069) tracks the wrong image. The fix, [#9396](https://github.com/tursodatabase/turso/pull/9396), remains open and unmerged as checked on 2026-10-13. The relevant before-image construction is unchanged in the [0.8.3 source](https://github.com/tursodatabase/turso/blob/v0.8.3/core/translate/upsert.rs#L1473).

## Workaround

Capturing project connections never execute `ON CONFLICT … DO UPDATE`. Within the existing project writer transaction, each relation reads its primary or business key, then executes checked `INSERT` for an absent row or checked `UPDATE … WHERE <key>` for a present row. Existing active watch boundaries are preserved. Capture stays full, foreign keys stay enabled, and the driver and schema are unchanged.

A permanent source check covers the capturing owners. A native integration test writes six product-sized intents, changes one from emitted to handedOff, and checks its persisted record plus the exact target ID and state in both captured images. Before the repair, the captured before-image named a different intent. After the repair, it names the target in state emitted; the after-image names that target in state handedOff.

The CDC columns are binary records. The test uses Turso's [documented decoding helpers](https://docs.turso.tech/tursodb/cdc), rather than assuming the blobs contain JSON text.

## Verification

```sh
cargo test -p message-board-storage --lib project_upsert_cdc_tests
cargo test -p collaboration-service --test intent_delivery_mcp
```

The first command checks the capturing source and actual native images. The second checks the board admission, renderer, stored push and delivery port; its external agent endpoint is a test stand-in.

## Removal check

On a Turso release containing #9396, run this small check on a disposable database with full capture, and run the permanent product-sized image test against a candidate conflict-update implementation:

```sql
PRAGMA capture_data_changes_conn('full');
CREATE TABLE image_check (id INTEGER PRIMARY KEY, state TEXT NOT NULL);
INSERT INTO image_check VALUES (1, 'old');
INSERT INTO image_check VALUES (1, 'new')
  ON CONFLICT(id) DO UPDATE SET state = excluded.state;
SELECT
  bin_record_json_object(table_columns_json_array('image_check'), before),
  bin_record_json_object(table_columns_json_array('image_check'), after)
FROM turso_cdc
WHERE table_name = 'image_check' AND change_type = 0;
```

Require exactly the original ID/value in the before-image and the new ID/value in the after-image. Also retain the length-changing, multiple-row product test and the admission/delivery regression. A successful statement alone is insufficient. Remove this workaround and its source prohibition only when the released engine passes those checks.
