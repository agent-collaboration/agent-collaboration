"""Materialize the controlled native skills into owned workspaces for one run.

The only writer in the native-capabilities apparatus besides evidence files: it creates
fresh workspaces outside the home directory and never touches an installation.
"""

import secrets
from pathlib import Path

from scripts.verification.acp_metadata_observation import save_raw
from scripts.verification.native_capabilities_models import FixtureRecord

FIXTURE_SOURCE = Path(__file__).parent / "fixtures" / "provider_native_capabilities"
FIXTURE_SKILLS = ("router-native-arguments", "router-native-second")
CANARY_PLACEHOLDER = "NATIVE_CAPABILITY_CANARY"
VISIBLE_FILE = "README.verification.txt"


def materialize_fixture(
    workspace: Path, empty_workspace: Path, record: Path
) -> FixtureRecord:
    """Create owned workspaces holding the controlled Claude project skills.

    Each skill body gets a fresh canary that appears nowhere else, so a reply carrying
    it can only come from the provider applying that skill.
    """
    for owned in (workspace, empty_workspace):
        if owned.exists():
            raise ValueError(f"owned workspace already exists: {owned}")
        if owned.resolve().is_relative_to(Path.home().resolve()):
            raise ValueError("owned workspaces stay outside the home directory")
        owned.mkdir(parents=True, mode=0o700)
    canaries: dict[str, str] = {}
    files: dict[str, Path] = {}
    for name in FIXTURE_SKILLS:
        canaries[name] = f"NATIVE_SKILL_{secrets.token_hex(8).upper()}"
        target = workspace / ".claude" / "skills" / name / "SKILL.md"
        target.parent.mkdir(parents=True, mode=0o700)
        body = (FIXTURE_SOURCE / name / "SKILL.md").read_text()
        if CANARY_PLACEHOLDER not in body:
            raise ValueError(f"fixture template {name} has no canary placeholder")
        _ = target.write_text(body.replace(CANARY_PLACEHOLDER, canaries[name]))
        files[name] = target
    visible = workspace / VISIBLE_FILE
    _ = visible.write_text("Visible workspace file for the @ file-only control.\n")
    fixture = FixtureRecord(
        workspace=workspace,
        empty_workspace=empty_workspace,
        visible_file=visible,
        skill_name=FIXTURE_SKILLS[0],
        skill_file=files[FIXTURE_SKILLS[0]],
        canary=canaries[FIXTURE_SKILLS[0]],
        second_skill_name=FIXTURE_SKILLS[1],
        second_skill_file=files[FIXTURE_SKILLS[1]],
        second_canary=canaries[FIXTURE_SKILLS[1]],
    )
    save_raw(record, fixture.model_dump_json(indent=2))
    return fixture
