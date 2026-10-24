"""Run external OB5 cases and retain redacted failure and cleanup outcomes.

Invoke through board_upgrade_journey --external-plan PLAN.json. No build,
central deployment/enrollment or production-process operation occurs here.
"""

import json
import traceback
from pathlib import Path

from .board_upgrade_external_drive import ExternalDrive
from .board_upgrade_external_plan import ExternalPlan


def observe_external(path: Path) -> int:
    plan = ExternalPlan.model_validate_json(path.read_bytes())
    drive = ExternalDrive(plan)
    outcome = "failed"
    try:
        for name in [
            "legacy-complete.sqlite",
            "role-history.sqlite",
            "mixed-watch-scope.sqlite",
        ]:
            drive.publication(name, name.removesuffix(".sqlite"))
        drive.publication("legacy-complete.sqlite", "before-publication", "before")
        drive.publication("legacy-complete.sqlite", "after-publication", "after")
        drive.collision()
        drive.claim()
        outcome = "passed"
        return 0
    except Exception as error:  # noqa: BLE001 -- redact authentication exceptions at the observation boundary
        # Admission errors can embed HTTP headers. Keep code locations and
        # checker evidence without retaining credential-bearing error text.
        _ = (drive.root / "failure.json").write_text(
            json.dumps(
                {
                    "type": type(error).__name__,
                    "check": str(error) if isinstance(error, AssertionError) else None,
                    "frames": [
                        {
                            "file": frame.filename,
                            "line": frame.lineno,
                            "function": frame.name,
                        }
                        for frame in traceback.extract_tb(error.__traceback__)
                    ],
                },
                indent=2,
            )
        )
        raise RuntimeError(
            "external OB5 stopped; inspect failure.json and checker records"
        ) from None
    finally:
        try:
            drive.close()
        except RuntimeError:
            outcome = "cleanupFailed"
            raise
        finally:
            drive.save(outcome)
        print(
            json.dumps(
                {
                    "outcome": outcome,
                    "passed": sum(c.passed for c in drive.checks.records),
                    "failed": sum(not c.passed for c in drive.checks.records),
                    "evidence": str(drive.root),
                }
            )
        )
