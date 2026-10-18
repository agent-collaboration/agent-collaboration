"""Fingerprint protected Codex configuration without a write or restore path."""

import hashlib
import json
import tomllib
import typing as t
from pathlib import Path

from pydantic import BaseModel, ConfigDict, Field, TypeAdapter

from scripts.verification.config_sections import (
    SectionChange,
    SectionFingerprint,
    compare_sections,
    fingerprint_sections,
)


class ConfigurationChanged(RuntimeError):
    """An owner-controlled configuration changed; leave it untouched."""


class ConfigurationModel(BaseModel):
    model_config: t.ClassVar[ConfigDict] = ConfigDict(frozen=True, extra="forbid")


class FileFingerprint(ConfigurationModel):
    path: Path
    sha256: str
    size: int
    modified_ns: int


class CodexHomeSnapshot(ConfigurationModel):
    profile: FileFingerprint
    config: FileFingerprint
    config_sections: tuple[SectionFingerprint, ...]


class ConfigurationObserved(ConfigurationModel):
    kind: t.Literal["observed"] = "observed"
    before: CodexHomeSnapshot
    after: CodexHomeSnapshot
    config_changes: tuple[SectionChange, ...]
    writer_attribution: t.Literal["unattributed"] = "unattributed"

    @property
    def unchanged(self) -> bool:
        return (
            self.before.profile == self.after.profile
            and self.before.config == self.after.config
        )

    @property
    def guard_passed(self) -> bool:
        return self.before.profile == self.after.profile and all(
            change.allowed for change in self.config_changes
        )


class ConfigurationUnreadable(ConfigurationModel):
    kind: t.Literal["unreadable"] = "unreadable"
    before: CodexHomeSnapshot
    error: str

    @property
    def unchanged(self) -> bool:
        return False

    @property
    def guard_passed(self) -> bool:
        return False


type ConfigurationObservation = t.Annotated[
    ConfigurationObserved | ConfigurationUnreadable, Field(discriminator="kind")
]


def _read_fingerprint(path: Path) -> tuple[FileFingerprint, bytes]:
    resolved = path.resolve(strict=True)
    before = resolved.stat()
    contents = resolved.read_bytes()
    after = resolved.stat()
    if before.st_mtime_ns != after.st_mtime_ns or before.st_size != after.st_size:
        raise ConfigurationChanged("configuration changed during fingerprint read")
    return FileFingerprint(
        path=resolved,
        sha256=hashlib.sha256(contents).hexdigest(),
        size=len(contents),
        modified_ns=after.st_mtime_ns,
    ), contents


def fingerprint(path: Path) -> FileFingerprint:
    return _read_fingerprint(path)[0]


def require_trusted_native_cwd(config: Path, cwd: Path) -> None:
    """Require an existing exact trust entry; never create or broaden one."""
    _, contents = _read_fingerprint(config)
    parsed = TypeAdapter(dict[str, object]).validate_python(
        t.cast(object, tomllib.loads(contents.decode()))
    )
    projects = TypeAdapter(dict[str, dict[str, object]]).validate_python(
        parsed.get("projects", {})
    )
    if (
        not cwd.is_dir()
        or projects.get(str(cwd.resolve()), {}).get("trust_level") != "trusted"
    ):
        raise ConfigurationChanged(
            "native cwd has no existing exact trusted entry; choose one without writing configuration"
        )


def snapshot_configuration(
    profile: Path, config: Path, references: tuple[str, ...] = ()
) -> CodexHomeSnapshot:
    config_fingerprint, contents = _read_fingerprint(config)
    return CodexHomeSnapshot(
        profile=fingerprint(profile),
        config=config_fingerprint,
        config_sections=fingerprint_sections(contents, references),
    )


def observe_configuration(
    before: CodexHomeSnapshot,
    profile: Path,
    config: Path,
    references: tuple[str, ...] = (),
) -> ConfigurationObservation:
    try:
        after = snapshot_configuration(profile, config, references)
    except (OSError, RuntimeError, ValueError) as error:
        return ConfigurationUnreadable(before=before, error=type(error).__name__)
    return ConfigurationObserved(
        before=before,
        after=after,
        config_changes=compare_sections(before.config_sections, after.config_sections),
    )


def require_read_only_configuration(observation: ConfigurationObservation) -> None:
    if not observation.guard_passed:
        raise ConfigurationChanged(
            "saved profile changed, configuration attribution failed or became unreadable; left untouched"
        )


def register_guard_identifiers(evidence: Path, value: object) -> None:
    """Record actual run identities in private evidence, never in Codex configuration."""
    identifiers: set[str] = set()

    def collect(item: object) -> None:
        if isinstance(item, dict):
            for key, child in (
                TypeAdapter(dict[str, object]).validate_python(item).items()
            ):
                if key in {
                    "sessionId",
                    "requestId",
                    "pushId",
                    "turnId",
                    "inputId",
                    "operationId",
                    "serviceId",
                    "serviceEpoch",
                } and isinstance(child, str):
                    identifiers.add(child)
                collect(child)
        elif isinstance(item, list):
            for child in TypeAdapter(list[object]).validate_python(item):
                collect(child)

    collect(value)
    path = evidence / "guard-identifiers.jsonl"
    with path.open("a") as output:
        _ = output.write(json.dumps(sorted(identifiers)) + "\n")
    path.chmod(0o600)


def configuration_references(
    evidence: Path, paths: tuple[Path, ...]
) -> tuple[str, ...]:
    references = {str(path.resolve()) for path in paths}
    ledger = evidence / "guard-identifiers.jsonl"
    if ledger.exists():
        for line in ledger.read_text().splitlines():
            values = TypeAdapter(
                list[str], config=ConfigDict(strict=True)
            ).validate_json(line)
            if any(not value for value in values):
                raise ValueError("invalid run identity ledger")
            references.update(values)
    return tuple(sorted(references))
