"""Drive board upgrade's real CLI/Tokio publication or offline native preparation.

Run as python -m scripts.verification.board_upgrade_journey --help.
With --publication-plan, drive dry-run, real upload, interrupted runs, owning
central reopen and same-identity reruns on supplied sealed copies (OB5/§5e).
With --observe-plan, drive the declared CLI and native observer against synthetic
fixtures and retain independent legacy-to-native assertions in private receipts.
Otherwise, inspect the source-entry gate without launching the product; its
receipt preserves unobserved obligations and does not establish import proof.
"""

import json
from pathlib import Path

from .board_upgrade_cli import observe
from .board_upgrade_external import observe_external
from .board_upgrade_publication import observe_publication
from .board_upgrade_views import UpgradeJourneyArguments
from .fleet_journey_entry import (
    EntryRequirement,
    entry_arguments,
    inspect_entry,
)

CASES = {
    "identity-history": "IDs one-to-one; seat events and resolutions become ordered Thread versions/participation interval changes; historical seat attribution comes from each event, and posted_from_activity becomes posted_as_role",
    "dense-order": "Each project's activity order maps to dense epoch-1 feed rows; a translated cutoff ends at the last row for its source activity",
    "metadata-description": "ProjectState is epoch 1 position 1, including empty topics; exact description stays in ProjectState, including empty/maximum-size/control-character text; directory has title only; declared Human owner/import origin/unknown time/no request and no invented ProjectCreated fact",
    "seat-intervals": "Each explicit rejoin closes the old interval as rejoined; handover closes an open target as promoted and the outgoing holder as replaced; closed role/note/join key stay unchanged",
    "root-attribution": "Creator join and main post import as one creation; either source cutoff maps to the post position; root posted_as_role comes only from its own posted_from_activity and remains null when absent",
    "mixed-bookmarks": "Shipped inbox selection keeps separate main and reply post sets; marks and watches preserve coverage",
    "watch-row-fidelity": "Legacy topic watches become project watches with translated cutoffs; preserve every active/inactive legacy thread-watch row and each membership, synthesize no thread-watch or opt-out row; after A's adc51bce correction the two native watch regressions pass unchanged with source oracle sets",
    "memberships": "Policy, expiry and generation retained; draining memberships ended at source's last recorded event time, even on reopened threads",
    "unknown-times": "Imported project and fleet records carry unknown time; rebuilding later keeps identical digests",
    "collisions": "Both conflicting IDs reported without renaming; real upload refused",
    "dry-run": "Only the report is written; old store untouched; a Router-held store refused",
    "idempotent-import": "Identical input yields alreadyImported; changed content importConflict; no second publication or grant after claim",
    "logical-digest": "Project digest includes exact legacy description/content, excludes import-run provenance and owner attribution; changed machine label replays alreadyImported, changed legacy content returns importConflict",
    "publication-recovery": "Interruptions on either side of publication resume the same import identity, with completion only after publication",
    "claim": "Imported project is unheld/notHeld; claim seeds the acknowledged copy before admitting writes in the next epoch",
}


def main() -> int:
    parser = entry_arguments(__doc__ or "Board upgrade entry gate")
    _ = parser.add_argument(
        "--external-plan",
        type=Path,
        help="Declared external OB5 endpoint, signed binaries and credential environment names; no deploy/enrollment",
    )
    _ = parser.add_argument(
        "--observe-plan",
        type=Path,
        help="Pinned D1a CLI/native observation plan JSON; synthetic fixtures only",
    )
    _ = parser.add_argument(
        "--publication-plan",
        type=Path,
        help="Pinned real CLI/Tokio plan with supplied sealed legacy copies",
    )
    _ = parser.add_argument(
        "--scenario",
        default="all",
        choices=[
            "all",
            "dry-run",
            "publication",
            "before-publication",
            "after-publication",
        ],
        help="Publication drive or bounded wrong-build control selection",
    )
    arguments = UpgradeJourneyArguments.model_validate(vars(parser.parse_args()))
    if (
        sum(
            plan is not None
            for plan in [
                arguments.observe_plan,
                arguments.publication_plan,
                arguments.external_plan,
            ]
        )
        > 1
    ):
        parser.error("choose one observation plan")
    if arguments.external_plan is not None:
        if arguments.scenario != "all":
            parser.error("external OB5 runs the complete declared sequence")
        return observe_external(arguments.external_plan)
    if arguments.publication_plan is not None:
        return observe_publication(arguments.publication_plan, arguments.scenario)
    if arguments.observe_plan is not None:
        return observe(arguments.observe_plan)
    if arguments.list_cases:
        print(json.dumps(CASES, indent=2))
        return 0
    if not arguments.expected_head or arguments.output_directory is None:
        parser.error(
            "--expected-head and --output-directory are required for the entry check"
        )
    return inspect_entry(
        arguments.expected_head,
        arguments.output_directory,
        EntryRequirement(
            journey="board-upgrade",
            package="board-legacy-upgrade",
            binary="codex-router-board-upgrade",
            cases=CASES,
        ),
    )


if __name__ == "__main__":
    raise SystemExit(main())
