"""Hash TOML table text and classify changes without retaining configuration values."""

import hashlib
import tomllib
import typing as t
from pathlib import Path

from pydantic import BaseModel, ConfigDict, TypeAdapter

type SectionKey = tuple[tuple[str, ...], int]


class SectionFingerprint(BaseModel):
    model_config: t.ClassVar[ConfigDict] = ConfigDict(frozen=True, extra="forbid")
    header: str
    table: tuple[str, ...]
    occurrence: int
    sha256: str
    names_run: bool


class SectionChange(BaseModel):
    model_config: t.ClassVar[ConfigDict] = ConfigDict(frozen=True, extra="forbid")
    header: str
    occurrence: int
    before_sha256: str | None
    after_sha256: str | None
    classification: t.Literal[
        "externalDesktopSection", "externalProjectSection", "runReference", "unexpected"
    ]

    @property
    def allowed(self) -> bool:
        return self.classification in {
            "externalDesktopSection",
            "externalProjectSection",
        }


def _quote_after(line: str, quote: str | None) -> str | None:
    position = 0
    while position < len(line):
        character = line[position]
        if quote is None:
            if character == "#":
                break
            if character in {"'", '"'}:
                quote = character * (
                    3 if line.startswith(character * 3, position) else 1
                )
                position += len(quote)
                continue
        elif quote.startswith('"') and character == "\\":
            position += 2
            continue
        elif line.startswith(quote, position):
            position += len(quote)
            quote = None
            continue
        position += 1
    return quote


def _table_header(line: str) -> str:
    stripped = line.lstrip()
    array = stripped.startswith("[[")
    closing = "]]" if array else "]"
    quote = None
    position = 2 if array else 1
    while position < len(stripped):
        character = stripped[position]
        if quote == '"' and character == "\\":
            position += 2
            continue
        if quote is not None:
            if character == quote:
                quote = None
        elif character in {"'", '"'}:
            quote = character
        elif stripped.startswith(closing, position):
            return stripped[: position + len(closing)]
        position += 1
    raise ValueError("table header has no closing bracket")


def _table_path(header: str) -> tuple[str, ...]:
    sentinel = "__verification_section_header__"
    parsed = t.cast(object, tomllib.loads(header + "\n" + sentinel + " = true\n"))
    path: list[str] = []
    current: object = parsed
    while isinstance(current, (dict, list)):
        if isinstance(current, list):
            entries = TypeAdapter(list[object]).validate_python(current)
            if len(entries) != 1:
                break
            current = entries[0]
            continue
        table = TypeAdapter(dict[str, object]).validate_python(current)
        if len(table) != 1:
            break
        key, value = next(iter(table.items()))
        if key == sentinel and value is True:
            return tuple(path)
        path.append(str(key))
        current = value
    raise ValueError("ambiguous table header")


def _contains_reference(text: str, reference: str) -> bool:
    if not reference.startswith("/"):
        return reference in text
    start = 0
    while (position := text.find(reference, start)) >= 0:
        ending = position + len(reference)
        if (
            ending == len(text)
            or text[ending].isspace()
            or text[ending] in "/\"',]}):;"
        ):
            return True
        start = ending
    return False


def _mentions(value: object, references: tuple[str, ...]) -> bool:
    if isinstance(value, str):
        return any(_contains_reference(value, reference) for reference in references)
    if isinstance(value, dict):
        table = TypeAdapter(dict[str, object]).validate_python(value)
        return any(
            _mentions(key, references) or _mentions(item, references)
            for key, item in table.items()
        )
    if isinstance(value, list):
        return any(
            _mentions(item, references)
            for item in TypeAdapter(list[object]).validate_python(value)
        )
    return False


def _related_project(table: tuple[str, ...], references: tuple[str, ...]) -> bool:
    if len(table) < 2 or table[0] != "projects":
        return False
    project = Path(table[1])
    if not project.is_absolute():
        return False
    project = project.resolve()
    for reference in references:
        candidate = Path(reference)
        if candidate.is_absolute():
            candidate = candidate.resolve()
            if (
                project == candidate
                or project in candidate.parents
                or candidate in project.parents
            ):
                return True
    return False


def fingerprint_sections(
    contents: bytes, references: tuple[str, ...]
) -> tuple[SectionFingerprint, ...]:
    text = contents.decode("utf-8")
    _ = tomllib.loads(text)
    references = tuple(reference for reference in references if reference)
    sections: list[tuple[str, str]] = []
    header = "<root>"
    lines: list[str] = []
    quote: str | None = None
    for line in text.splitlines(keepends=True):
        if quote is None and line.lstrip().startswith("["):
            sections.append((header, "".join(lines)))
            header, lines = _table_header(line), []
        lines.append(line)
        quote = _quote_after(line, quote)
    sections.append((header, "".join(lines)))
    result: list[SectionFingerprint] = []
    occurrences: dict[tuple[str, ...], int] = {}
    for header, raw_text in sections:
        table = () if header == "<root>" else _table_path(header)
        occurrence = occurrences.get(table, 0)
        occurrences[table] = occurrence + 1
        # Parsing the section also catches escaped run references in keys/values.
        parsed = tomllib.loads(raw_text)
        names_run = (
            any(_contains_reference(raw_text, reference) for reference in references)
            or _mentions(parsed, references)
            or _related_project(table, references)
        )
        result.append(
            SectionFingerprint(
                header=header,
                table=table,
                occurrence=occurrence,
                sha256=hashlib.sha256(raw_text.encode()).hexdigest(),
                names_run=names_run,
            )
        )
    return tuple(result)


def _array_element_owners(
    sections: tuple[SectionFingerprint, ...],
) -> dict[SectionKey, tuple[SectionKey, ...]]:
    active_arrays: dict[tuple[str, ...], int] = {}
    owners: dict[SectionKey, tuple[SectionKey, ...]] = {}
    for section in sections:
        if section.header.startswith("[["):
            # A new parent element ends every nested array's previous scope.
            active_arrays = {
                path: occurrence
                for path, occurrence in active_arrays.items()
                if path[: len(section.table)] != section.table
            }
            active_arrays[section.table] = section.occurrence
        owners[(section.table, section.occurrence)] = tuple(
            (path, occurrence)
            for path, occurrence in sorted(active_arrays.items())
            if section.table[: len(path)] == path
        )
    return owners


def compare_sections(
    before: tuple[SectionFingerprint, ...], after: tuple[SectionFingerprint, ...]
) -> tuple[SectionChange, ...]:
    old = {(section.table, section.occurrence): section for section in before}
    new = {(section.table, section.occurrence): section for section in after}
    old_owners = _array_element_owners(before)
    new_owners = _array_element_owners(after)
    changes: list[SectionChange] = []
    for table, occurrence in sorted(old.keys() | new.keys()):
        left, right = old.get((table, occurrence)), new.get((table, occurrence))
        if (
            left is not None
            and right is not None
            and left.sha256 == right.sha256
            and old_owners[(table, occurrence)] == new_owners[(table, occurrence)]
        ):
            continue
        representative = right or left
        assert representative is not None
        classification: t.Literal[
            "externalDesktopSection",
            "externalProjectSection",
            "runReference",
            "unexpected",
        ] = "unexpected"
        if any(section.names_run for section in [left, right] if section is not None):
            classification = "runReference"
        elif table and table[0] == "desktop":
            classification = "externalDesktopSection"
        elif (
            len(table) >= 2 and table[0] == "projects" and Path(table[1]).is_absolute()
        ):
            classification = "externalProjectSection"
        changes.append(
            SectionChange(
                header=representative.header,
                occurrence=occurrence,
                before_sha256=left.sha256 if left else None,
                after_sha256=right.sha256 if right else None,
                classification=classification,
            )
        )
    return tuple(changes)
