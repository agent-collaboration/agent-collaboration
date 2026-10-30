# S2 sensitivity controls

These patches intentionally break one named contract. They are test inputs,
never product features or runtime switches. At the declared S2 head, apply one
from the workspace root, run the **entire package**, then reverse that same patch:

```sh
git apply crates/agent-collaboration-tui/tests/seeded_wrong_builds/duplicate-key.patch
env -u NO_COLOR cargo nextest run --profile ci -p agent-collaboration-tui
git apply -R crates/agent-collaboration-tui/tests/seeded_wrong_builds/duplicate-key.patch
```

| Patch | Required failure |
| --- | --- |
| `duplicate-key.patch` | Duplicate scope keys accepted (last binding wins). |
| `restore-index.patch` | Reordering restores beta instead of captured alpha. |
| `wide-menu-glyph.patch` | Menu glyph is two cells in the current width table. |
| `permissive-frame-reader.patch` | Unknown CSI and OSC 8 accepted; CSI K leaves stale row content. |
| `separated-burst.patch` | The event acknowledgements have draw counts 1, 2, 3 instead of 1, 1, 1. |

The core controls first failed together in a full package run (40 passed,
3 failed, 1 skipped). The harness controls first failed together in a full run
with normal color output (45 passed, 4 failed, 1 skipped). Compile errors and the
earlier inherited-NO_COLOR style failure are not counted as sensitivity evidence.
The correct S2 build passes all 49 tests (1 existing ignored measurement).

Check each patch applies before replay. Later slices must refresh a patch if its
owning model changes, then re-establish its named full-package failure at the S5
target. A failed patch application is an evidence gap, never a passing control.
