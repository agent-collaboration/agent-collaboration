"""Catalogue setup checks; these never build or start a Worker."""

from pathlib import Path

import pytest

from scripts.verification.central_worker_scenarios import (
    workerd_command,
    workerd_scenarios,
)


@pytest.mark.parametrize(
    "mode",
    ["mailbox-deadline", "installation-directory", "delivery-config-refusals"],
)
def test_added_modes_match_the_current_worker_entry(mode: str) -> None:
    repository = Path(__file__).resolve().parents[2]
    entry = (
        repository
        / "crates/collaboration-central-tokio/examples/session_scope_workerd.rs"
    )
    results = entry.with_suffix("") / "case_results.rs"
    [(selected, scenario)] = workerd_scenarios((mode,))

    assert selected == mode
    assert f'Some("{mode}")' in entry.read_text(encoding="utf-8")
    assert scenario.marker in results.read_text(encoding="utf-8")
    assert scenario.minimum_reaped_processes == 1
    command = workerd_command(mode, "node", Path("miniflare"), Path("state"), 12345)
    assert command[-5:] == ["node", "miniflare", "state", "12345", mode]
    assert "session_scope_workerd" in command
