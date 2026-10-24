"""Declare one OB7 target without storing credentials or inferring authority."""

import re
import typing as t
import urllib.parse
from pathlib import Path
from uuid import UUID

from pydantic import BaseModel, ConfigDict, Field, model_validator


class StrictModel(BaseModel):
    model_config: t.ClassVar[ConfigDict] = ConfigDict(
        extra="forbid", frozen=True, hide_input_in_errors=True
    )


class Binary(StrictModel):
    path: Path
    sha256: str = Field(pattern=r"^[0-9a-f]{64}$")


class Credentials(StrictModel):
    machine: str
    access_id: str
    access_secret: str

    @model_validator(mode="after")
    def names_only(self) -> t.Self:
        if any(
            not re.fullmatch(r"[A-Z][A-Z0-9_]*", name)
            for name in [self.machine, self.access_id, self.access_secret]
        ):
            raise ValueError("credentials are environment names only")
        return self


class EnrolledInstallation(StrictModel):
    # A stopped, dedicated staging fixture root, never a production Router home.
    seed_root: Path
    installation: UUID
    peer: UUID
    credentials: Credentials


class DeployCommand(StrictModel):
    executable: Binary
    arguments: list[str]
    cwd: Path
    namespace_pin: Path
    source_files: list[Binary] = Field(min_length=1)


class LocalTarget(StrictModel):
    kind: t.Literal["localWorkerd"]
    node: Path
    miniflare: Path
    native_probe: Binary
    bundle: Path
    bundle_js_sha256: str = Field(pattern=r"^[0-9a-f]{64}$")
    bundle_wasm_sha256: str = Field(pattern=r"^[0-9a-f]{64}$")


class StagingTarget(StrictModel):
    kind: t.Literal["staging"]
    origin: str
    script: str
    deployed_version: str = Field(min_length=1)
    deployed_source_head: str = Field(pattern=r"^[0-9a-f]{40}$")
    access_policy_id: str = Field(min_length=1)
    owner_token_environment: str = Field(pattern=r"^[A-Z][A-Z0-9_]*$")
    receiver: UUID
    relay: UUID
    installations: tuple[EnrolledInstallation, EnrolledInstallation]
    alternate_origins: list[str] = Field(min_length=1)
    # Independent, complete cf inventory receipt; list origins must match exactly.
    address_inventory: Path
    native_probe: Binary
    deploy: DeployCommand | None = None

    @model_validator(mode="after")
    def isolated_origins(self) -> t.Self:
        for origin in [self.origin, *self.alternate_origins]:
            parsed = urllib.parse.urlsplit(origin)
            if (
                parsed.scheme != "https"
                or not parsed.hostname
                or parsed.username
                or parsed.password
                or parsed.query
                or parsed.fragment
                or parsed.path not in {"", "/"}
                or parsed.port in {8787, 8788, 43127}
            ):
                raise ValueError("declare credential-free HTTPS origins only")
        if self.origin in self.alternate_origins or len(
            set(self.alternate_origins)
        ) != len(self.alternate_origins):
            raise ValueError(
                "alternate origins must be distinct and exclude the Access origin"
            )
        first, second = self.installations
        if (
            first.installation == second.installation
            or first.peer == second.peer
            or first.seed_root == second.seed_root
        ):
            raise ValueError("two distinct pre-enrolled installations are required")
        if (
            self.receiver.version != 7
            or self.relay.version != 7
            or self.receiver == self.relay
        ):
            raise ValueError("declare distinct recorded UUIDv7 receiver and relay")
        return self


type Target = t.Annotated[LocalTarget | StagingTarget, Field(discriminator="kind")]
type Case = t.Literal[
    "access", "replication-deploy", "takeover-recovery", "idle-resume"
]


class Plan(StrictModel):
    repository: Path
    expected_head: str = Field(pattern=r"^[0-9a-f]{40}$")
    evidence: Path
    runtime_parent: Path
    artifact_root: Path
    target: Target
    cases: list[Case] = Field(min_length=1)
    idle_seconds: int = Field(default=120, ge=1, le=43200)

    @model_validator(mode="after")
    def bounded_plan(self) -> t.Self:
        if len(set(self.cases)) != len(self.cases):
            raise ValueError("select each case once")
        for path in [
            self.repository,
            self.evidence,
            self.runtime_parent,
            self.artifact_root,
        ]:
            if not path.is_absolute() or path.resolve() != path:
                raise ValueError("paths must be canonical and absolute")
        if (
            self.evidence == self.repository
            or self.evidence.is_relative_to(self.repository)
            or not self.runtime_parent.is_relative_to(Path("/private/tmp"))
        ):
            raise ValueError(
                "evidence is private outside the checkout; runtime roots belong to /private/tmp"
            )
        if (
            isinstance(self.target, StagingTarget)
            and any(
                case in self.cases
                for case in ["replication-deploy", "takeover-recovery"]
            )
            and self.target.deploy is None
        ):
            raise ValueError(
                "deploy cases require the explicitly commissioned guarded deploy command"
            )
        return self


class SessionState(StrictModel):
    sessionId: UUID
    noticeCursor: int


class AddressInventory(StrictModel):
    script: str
    deployed_version: str
    alternate_origins: list[str]
    # Exact API response hashes retained by the independent inventory collector.
    inventory_sources: list[Binary] = Field(min_length=1)
    complete: t.Literal[True]
