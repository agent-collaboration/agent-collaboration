"""Fingerprint protected Codex configuration without a write or restore path."""

import hashlib
import typing as t
from pathlib import Path

from pydantic import BaseModel, ConfigDict, Field


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


class ConfigurationObserved(ConfigurationModel):
    kind: t.Literal["observed"] = "observed"
    before: CodexHomeSnapshot
    after: CodexHomeSnapshot

    @property
    def unchanged(self) -> bool:
        return self.before == self.after


class ConfigurationUnreadable(ConfigurationModel):
    kind: t.Literal["unreadable"] = "unreadable"
    before: CodexHomeSnapshot
    error: str

    @property
    def unchanged(self) -> bool:
        return False


type ConfigurationObservation = t.Annotated[
    ConfigurationObserved | ConfigurationUnreadable, Field(discriminator="kind")
]


def fingerprint(path: Path) -> FileFingerprint:
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
    )


def snapshot_configuration(profile: Path, config: Path) -> CodexHomeSnapshot:
    return CodexHomeSnapshot(profile=fingerprint(profile), config=fingerprint(config))


def observe_configuration(
    before: CodexHomeSnapshot, profile: Path, config: Path
) -> ConfigurationObservation:
    try:
        after = snapshot_configuration(profile, config)
    except (OSError, RuntimeError) as error:
        return ConfigurationUnreadable(before=before, error=type(error).__name__)
    return ConfigurationObserved(before=before, after=after)


def require_unchanged(observation: ConfigurationObservation) -> None:
    if not observation.unchanged:
        raise ConfigurationChanged(
            "saved debug profile or config.toml changed or became unreadable; left untouched"
        )
