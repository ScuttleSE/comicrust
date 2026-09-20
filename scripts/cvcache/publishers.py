"""Publisher whitelist / blacklist loading (phase-21 Task B).

The file format matches the reference `sqlite_cv_pipeline_1.1.0.py`
files: a `# ID, Name` header, `#` comment lines, then `ID, Name` rows.
The `Publisher_Master_List_*` and `Publisher_List_Top 75%*` files use
the same format and load through the same reader; a caller curates a
black/white list from them.

A whitelist keeps only its ids. A blacklist drops its ids. When both
are present the whitelist runs first, then the blacklist removes from
what the whitelist kept.
"""

from __future__ import annotations

from pathlib import Path


def load_publisher_ids(path: Path) -> set[int]:
    """Reads the `ID, Name` lines of one list file. A blank line, a
    `#` comment, or a line with no leading integer is skipped."""
    ids: set[int] = set()
    text = path.read_text(encoding="utf-8", errors="replace")
    for line in text.splitlines():
        stripped = line.strip()
        if not stripped or stripped.startswith("#"):
            continue
        head = stripped.split(",", 1)[0].strip()
        try:
            ids.add(int(head))
        except ValueError:
            continue
    return ids


class PublisherFilter:
    """Decides whether a publisher id passes. `None` on either side
    means that side is not in force."""

    def __init__(
        self,
        whitelist: set[int] | None = None,
        blacklist: set[int] | None = None,
    ) -> None:
        self.whitelist = whitelist
        self.blacklist = blacklist

    @classmethod
    def from_files(
        cls,
        whitelist_path: Path | None = None,
        blacklist_path: Path | None = None,
    ) -> "PublisherFilter":
        whitelist = (
            load_publisher_ids(whitelist_path)
            if whitelist_path is not None
            else None
        )
        blacklist = (
            load_publisher_ids(blacklist_path)
            if blacklist_path is not None
            else None
        )
        return cls(whitelist, blacklist)

    def allows(self, publisher_id: int | None) -> bool:
        if self.whitelist is not None:
            if publisher_id is None or publisher_id not in self.whitelist:
                return False
        if self.blacklist is not None and publisher_id in self.blacklist:
            return False
        return True

    @property
    def in_force(self) -> bool:
        return self.whitelist is not None or self.blacklist is not None
