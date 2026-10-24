"""Declared external OB5 target; credentials are environment names, never values."""

import os
import re
import typing as t
import urllib.parse
import uuid
from functools import cached_property
from pathlib import Path

from pydantic import BaseModel, ConfigDict, Field, field_validator


class FrozenArtifact(BaseModel):
    model_config: t.ClassVar[ConfigDict] = ConfigDict(extra="ignore", frozen=True)
    path: Path
    sourcePath: Path
    sha256: str = Field(pattern=r"^[0-9a-f]{64}$")
    sourcePin: str = Field(pattern=r"^[0-9a-f]{40}$")
    identifier: str
    team: str
    strictSignatureVerified: bool


class FrozenArtifacts(BaseModel):
    model_config: t.ClassVar[ConfigDict] = ConfigDict(extra="ignore", frozen=True)
    pin: str = Field(pattern=r"^[0-9a-f]{40}$")
    artifactDirectory: Path
    artifacts: list[FrozenArtifact]


class ExternalPlan(BaseModel):
    model_config: t.ClassVar[ConfigDict] = ConfigDict(extra="forbid", frozen=True)
    repository: Path
    expected_head: str = Field(pattern=r"^[0-9a-f]{40}$")
    artifact_receipt: Path
    endpoint: str
    runtime_parent: Path
    output_directory: Path
    authentication: t.Literal["ownerAccess", "localFixture"]
    # localFixture uses the already-running, privately owned fixture. It never
    # enrolls a staging installation or starts/deploys an external central.
    fixture_assertion: Path | None = None
    machine_endpoint: str | None = None
    machine_environment: dict[str, str]
    timeout_seconds: int = Field(default=300, ge=30, le=3600)

    @cached_property
    def receipt(self) -> FrozenArtifacts:
        return FrozenArtifacts.model_validate_json(self.artifact_receipt.read_bytes())

    def artifact(self, name: str) -> FrozenArtifact:
        matches = [
            artifact
            for artifact in self.receipt.artifacts
            if artifact.path.name == name
        ]
        if len(matches) != 1:
            raise ValueError("frozen receipt must name each used artifact exactly once")
        return matches[0]

    @property
    def artifacts(self) -> Path:
        return self.receipt.artifactDirectory

    @property
    def binary(self) -> Path:
        return self.artifact("codex-router-board-upgrade").path

    @property
    def binary_sha256(self) -> str:
        return self.artifact("codex-router-board-upgrade").sha256

    @property
    def native_binary(self) -> Path:
        return self.artifact("board-upgrade-native-observer").path

    @property
    def native_sha256(self) -> str:
        return self.artifact("board-upgrade-native-observer").sha256

    @property
    def launcher_sha256(self) -> str:
        return self.artifact("board-debug-runtime").sha256

    @property
    def cli_sha256(self) -> str:
        return self.artifact("agent-collaboration").sha256

    @field_validator("machine_environment")
    @classmethod
    def environment_names(cls, value: dict[str, str]) -> dict[str, str]:
        required = {
            "installation",
            "peer",
            "receiver",
            "relay",
            "credential",
            "access_client_id",
            "access_client_secret",
        }
        if set(value) != required or any(
            re.fullmatch(r"[A-Z][A-Z0-9_]*", name) is None for name in value.values()
        ):
            raise ValueError("machine_environment needs seven environment names")
        return value

    def machine(self) -> dict[str, str]:
        values: dict[str, str] = {}
        for field, name in self.machine_environment.items():
            value = os.environ.get(name)
            if not value or "\r" in value or "\n" in value:
                raise ValueError(f"credential input unavailable: {name}")
            values[field] = value
        for field in ["installation", "peer", "receiver", "relay"]:
            try:
                identity = uuid.UUID(values[field])
                if str(identity) != values[field] or (
                    field != "installation" and identity.version != 7
                ):
                    raise ValueError("noncanonical")
            except ValueError:
                raise ValueError(
                    f"invalid machine identity input: {self.machine_environment[field]}"
                ) from None
        return values

    def validate_endpoint(self) -> None:
        url = urllib.parse.urlsplit(self.endpoint)
        if (
            url.username
            or url.password
            or url.query
            or url.fragment
            or url.path not in {"", "/"}
        ):
            raise ValueError("declare an origin without credentials, path or query")
        local = url.scheme == "http" and url.hostname == "127.0.0.1" and url.port
        if url.port in {8787, 8788, 43127}:
            raise ValueError("protected port")
        if self.authentication == "localFixture":
            if not local or self.fixture_assertion is None:
                raise ValueError(
                    "localFixture needs loopback and an owner-private assertion"
                )
            if self.machine_endpoint is not None:
                machine = urllib.parse.urlsplit(self.machine_endpoint)
                if (
                    machine.scheme != "http"
                    or machine.hostname != "127.0.0.1"
                    or machine.port in {None, 8787, 8788, 43127}
                    or machine.path != "/v1/session"
                    or machine.username
                    or machine.password
                    or machine.query
                    or machine.fragment
                ):
                    raise ValueError(
                        "fixture machine edge must be explicit isolated loopback"
                    )
        elif (
            url.scheme != "https"
            or not url.hostname
            or self.fixture_assertion is not None
        ):
            raise ValueError(
                "ownerAccess needs a declared HTTPS origin and no fixture assertion"
            )
        elif self.machine_endpoint is not None:
            raise ValueError(
                "ownerAccess machine session must use the same declared origin"
            )
