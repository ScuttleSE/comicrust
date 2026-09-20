"""The MCL reader (ADR-038), a port of the read half of
`crates/cr-scrape/src/cache/mcl.rs`.

The reader accepts what the source writer really produces: the number
list keeps its trailing comma, the `.&@1`/`.&@2` escapes reverse to a
comma and a semicolon, a quoted number list is accepted, and a comma
that a space follows is not a separator. The scripts read MCL; they do
not write it.
"""

from __future__ import annotations

from dataclasses import dataclass, field
from typing import Iterator, TextIO

_ESC_COMMA = ".&@1"
_ESC_SEMI = ".&@2"
_MAX_ERRORS = 20


@dataclass
class MclIssue:
    issue_id: int
    issue_number: str


@dataclass
class MclVolume:
    volume_id: int
    issues: list[MclIssue] = field(default_factory=list)


@dataclass
class MclReport:
    date: str = ""
    volumes: int = 0
    issues: int = 0
    skipped: int = 0
    errors: list = field(default_factory=list)


def read(reader: TextIO) -> Iterator[tuple[MclVolume | None, MclReport]]:
    """Yields `(volume, report)` for each parsed volume line, then a
    final `(None, report)` with the totals. The report is the same
    running object each time."""
    report = MclReport()
    for index, raw in enumerate(reader):
        line = raw.rstrip("\r\n")
        if not line.strip():
            continue
        if index == 0:
            date = parse_header(line)
            if date is not None:
                report.date = date
                continue
        try:
            volume = parse_volume_line(line)
        except ValueError as error:
            report.skipped += 1
            if len(report.errors) < _MAX_ERRORS:
                report.errors.append((index + 1, str(error)))
            continue
        report.volumes += 1
        report.issues += len(volume.issues)
        yield volume, report
    yield None, report


def parse_header(line: str) -> str | None:
    stripped = line.strip()
    if not stripped.startswith("Missing"):
        return None
    rest = stripped[len("Missing"):]
    if rest.startswith(";"):
        rest = rest[1:]
    return rest.strip()


def parse_volume_line(line: str) -> MclVolume:
    fields = line.strip().split(";", 2)
    if len(fields) < 2:
        raise ValueError("no issue-id field")
    if len(fields) < 3:
        raise ValueError("no issue-number field")
    id_field, issues_field, numbers_field = fields
    id_field = id_field.strip()
    try:
        volume_id = int(id_field)
    except ValueError:
        raise ValueError(f"volume id {id_field!r} is not a number")
    ids = []
    for token in issues_field.split(","):
        token = token.strip()
        if not token:
            continue
        try:
            ids.append(int(token))
        except ValueError:
            raise ValueError(f"issue id {token!r} is not a number")
    numbers = _split_numbers(numbers_field)
    issues = []
    for i, issue_id in enumerate(ids):
        number = numbers[i] if i < len(numbers) else ""
        issues.append(MclIssue(issue_id, _unescape(number)))
    return MclVolume(volume_id, issues)


def _split_numbers(field_text: str) -> list[str]:
    """Splits the number list on a comma that a space does not follow.
    A wrapping pair of double quotes is stripped first."""
    text = field_text.strip()
    if len(text) >= 2 and text.startswith('"') and text.endswith('"'):
        text = text[1:-1]
    out: list[str] = []
    current: list[str] = []
    i = 0
    while i < len(text):
        char = text[i]
        if char == ",":
            following = text[i + 1] if i + 1 < len(text) else ""
            if following == " ":
                current.append(char)
            else:
                out.append("".join(current))
                current = []
        else:
            current.append(char)
        i += 1
    out.append("".join(current))
    # The list ends with a trailing comma, so drop the empty tail.
    if out and out[-1] == "":
        out.pop()
    return out


def _unescape(number: str) -> str:
    return number.replace(_ESC_COMMA, ",").replace(_ESC_SEMI, ";")
