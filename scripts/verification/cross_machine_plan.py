"""Explicit binary, process, credential-name and transport bindings for OB4."""

import re
import typing as t
import urllib.parse
from pathlib import Path

from pydantic import BaseModel, ConfigDict, Field, model_validator

from .typed_observations import SessionView


class BinaryBinding(BaseModel):
    model_config: t.ClassVar[ConfigDict] = ConfigDict(extra="forbid", frozen=True)
    path: Path
    sha256: str = Field(pattern=r"^[0-9a-f]{64}$")
    source_head: str = Field(pattern=r"^[0-9a-f]{40}$")
    transfer_source: Path | None = None


class CredentialNames(BaseModel):
    model_config: t.ClassVar[ConfigDict] = ConfigDict(extra="forbid", frozen=True)
    machine: str
    access_id: str
    access_secret: str

    @model_validator(mode="after")
    def environment_names_only(self) -> t.Self:
        for name in [self.machine, self.access_id, self.access_secret]:
            if not re.fullmatch(r"[A-Z][A-Z0-9_]*", name):
                raise ValueError("credentials must be environment names, never values")
        return self


class CentralSession(BaseModel):
    model_config: t.ClassVar[ConfigDict] = ConfigDict(extra="forbid", frozen=True)
    endpoint: str
    credentials: CredentialNames
    peer: str
    receiver: str
    relay: str

    @model_validator(mode="after")
    def session_origin(self) -> t.Self:
        url = urllib.parse.urlsplit(self.endpoint)
        if (
            url.scheme not in {"http", "https", "ws", "wss"}
            or not url.hostname
            or url.username
            or url.password
            or url.query
            or url.fragment
            or url.path != "/v1/session"
            or url.port in {8787, 8788, 43127}
        ):
            raise ValueError(
                "session endpoint must be a declared credential-free machine route"
            )
        return self


class FixtureOwnerAccess(BaseModel):
    model_config: t.ClassVar[ConfigDict] = ConfigDict(extra="forbid", frozen=True)
    kind: t.Literal["localFixture"]
    assertion_file: Path


class NamedOwnerAccess(BaseModel):
    model_config: t.ClassVar[ConfigDict] = ConfigDict(extra="forbid", frozen=True)
    kind: t.Literal["ownerAccess"]
    token_environment: str = Field(pattern=r"^[A-Z][A-Z0-9_]*$")


class CentralTarget(BaseModel):
    model_config: t.ClassVar[ConfigDict] = ConfigDict(extra="forbid", frozen=True)
    endpoint: str
    authentication: t.Annotated[
        FixtureOwnerAccess | NamedOwnerAccess, Field(discriminator="kind")
    ]

    @model_validator(mode="after")
    def owner_origin(self) -> t.Self:
        url = urllib.parse.urlsplit(self.endpoint)
        if (
            not url.hostname
            or url.username
            or url.password
            or url.query
            or url.fragment
            or url.path not in {"", "/"}
            or url.port in {8787, 8788, 43127}
        ):
            raise ValueError("declare one credential-free owner HTTP origin")
        if isinstance(self.authentication, FixtureOwnerAccess):
            if url.scheme != "http" or url.hostname != "127.0.0.1" or not url.port:
                raise ValueError("fixture owner access is isolated loopback only")
        elif url.scheme != "https":
            raise ValueError("ownerAccess requires HTTPS")
        return self


class ProviderExecutable(BaseModel):
    model_config: t.ClassVar[ConfigDict] = ConfigDict(extra="forbid", frozen=True)
    path: Path
    sha256: str = Field(pattern=r"^[0-9a-f]{64}$")
    version: str


class RecipientProvider(BaseModel):
    model_config: t.ClassVar[ConfigDict] = ConfigDict(extra="forbid", frozen=True)
    adapter: ProviderExecutable
    claude: ProviderExecutable
    configuration_directory: Path
    api_key_environment: str | None = Field(default=None, pattern=r"^[A-Z][A-Z0-9_]*$")
    model: t.Literal["claude-haiku-5-5"] = "claude-haiku-5-5"


class InstallationPlan(BaseModel):
    model_config: t.ClassVar[ConfigDict] = ConfigDict(extra="forbid", frozen=True)
    name: str = Field(pattern=r"^[a-z][a-z0-9-]{0,30}$")
    task_directory: Path
    temporary_parent: Path
    launcher: BinaryBinding
    cli: BinaryBinding
    port: int = Field(ge=0, le=65535)
    ssh_host: str | None = None
    remote_python: str = "python3"
    session: CentralSession | None = None
    recipient_provider: RecipientProvider | None = None

    @model_validator(mode="after")
    def private_and_unprotected(self) -> t.Self:
        if self.port in {8787, 8788, 43127}:
            raise ValueError("protected production/debug port")
        for path in [
            self.task_directory,
            self.temporary_parent,
            self.launcher.path,
            self.cli.path,
        ]:
            if not path.is_absolute() or ".." in path.parts:
                raise ValueError(
                    "all installation paths must be canonical absolute paths"
                )
        if (
            not self.task_directory.is_relative_to(self.temporary_parent)
            or self.task_directory == self.temporary_parent
        ):
            raise ValueError(
                "task directory must be below its declared temporary parent"
            )
        if (
            ".codex" in self.task_directory.parts
            or ".codex-router" in self.task_directory.parts
        ):
            raise ValueError("normal account state is not an installation root")
        if self.ssh_host is not None and not re.fullmatch(
            r"[A-Za-z0-9][A-Za-z0-9_.@-]*", self.ssh_host
        ):
            raise ValueError("SSH host must be one literal destination")
        if self.ssh_host is not None:
            for binary in [self.launcher, self.cli]:
                if not binary.path.is_relative_to(self.task_directory):
                    raise ValueError(
                        "remote artifacts must be inside the one task directory"
                    )
                if (
                    binary.transfer_source is None
                    or not binary.transfer_source.is_absolute()
                ):
                    raise ValueError(
                        "remote artifact transfer requires its exact local source"
                    )
        if self.recipient_provider is not None:
            configuration = self.recipient_provider.configuration_directory
            if self.ssh_host is not None and not configuration.is_relative_to(
                self.task_directory
            ):
                raise ValueError(
                    "remote recipient configuration must remain in its one task root"
                )
            if (
                not configuration.is_absolute()
                or ".." in configuration.parts
                or not configuration.is_relative_to(self.temporary_parent)
                or configuration == self.temporary_parent
                or any(
                    part in configuration.parts
                    for part in [".claude", ".codex", ".codex-router"]
                )
            ):
                raise ValueError(
                    "recipient configuration must be a declared private task path"
                )
        return self


class CrossMachinePlan(BaseModel):
    model_config: t.ClassVar[ConfigDict] = ConfigDict(extra="forbid", frozen=True)
    repository: Path
    output_directory: Path
    installations: tuple[InstallationPlan, InstallationPlan]
    mode: t.Literal["readiness", "local-conformance", "matrix"]
    central_binary: BinaryBinding | None = None
    importer_binary: BinaryBinding | None = None
    central_target: CentralTarget | None = None
    caller: SessionView | None = None
    recipient_cases: (
        list[
            t.Literal[
                "both-directions",
                "local-independence",
                "held-copy-ack",
                "distinct-handoffs",
                "receipt-retention",
                "offline-retry",
                "cancellation",
                "wakes",
            ]
        ]
        | None
    ) = Field(default=None, min_length=1)
    # No staging activation is implicit in a plan or in a successful preflight.
    allow_external_central: bool = False

    @model_validator(mode="after")
    def distinct_installations(self) -> t.Self:
        first, second = self.installations
        if first.name == second.name or (first.ssh_host, first.task_directory) == (
            second.ssh_host,
            second.task_directory,
        ):
            raise ValueError("two distinct installation identities/roots are required")
        if self.mode == "readiness" and any(
            item.session is not None for item in self.installations
        ):
            raise ValueError("readiness-only qualification must not contact a central")
        if self.mode == "local-conformance" and (
            self.central_binary is None
            or any(item.ssh_host for item in self.installations)
        ):
            raise ValueError(
                "local conformance needs two local installations and a declared central binary"
            )
        if self.mode == "matrix" and (
            self.central_target is None
            or any(item.session is None for item in self.installations)
        ):
            raise ValueError(
                "matrix requires an explicit owner origin and both enrolled machine session declarations"
            )
        if not self.allow_external_central and any(
            item.session is not None for item in self.installations
        ):
            raise ValueError(
                "external central use needs an explicitly authorized target plan"
            )
        return self
