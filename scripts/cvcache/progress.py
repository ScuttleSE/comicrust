"""Verbose progress output for the `update` command (ADR-075).

A live per-endpoint display driven by `rich`: current page, rows
fetched and staged, the rolling hourly request budget, and a wait
countdown when the cap is reached. A final summary table closes the
run. When `rich` is not installed, or `--quiet` is set, the same
information prints as plain text lines instead, so a redirected run or
a bare machine never fails for want of the dependency.

The core fetch/merge pipeline in `update.py` is standard-library only;
this module is the one place that reaches for `rich`.
"""

from __future__ import annotations

try:
    from rich.console import Console
    from rich.table import Table
    from rich.live import Live
    from rich.panel import Panel

    _HAVE_RICH = True
except ImportError:  # pragma: no cover - exercised only without rich
    _HAVE_RICH = False


def make_display(endpoints, max_per_hour: int, quiet: bool):
    """Returns a display object with on_preflight(estimates),
    on_endpoint_start(endpoint), on_page(report, total),
    on_wait(endpoint, seconds), and finish(reports). Picks the rich or
    the plain implementation."""
    if quiet or not _HAVE_RICH:
        if not quiet and not _HAVE_RICH:
            print(
                "note: install 'rich' (pip install rich) for the live "
                "progress display; falling back to plain output."
            )
        return _PlainDisplay(max_per_hour)
    return _RichDisplay(endpoints, max_per_hour)


def _budget_line(report, changed, max_per_hour: int) -> str:
    pct = f"{100 * report.fetched // changed}%" if changed else "—"
    return (
        f"{report.endpoint:<11} page {report.pages:>4}  "
        f"fetched {report.fetched:>6}/{changed:<7} ({pct})  "
        f"staged {report.staged:>6}"
    )


class _PlainDisplay:
    def __init__(self, max_per_hour: int):
        self.max_per_hour = max_per_hour
        self.changed = {}

    def on_preflight(self, estimates):
        self.changed = {e.endpoint: e.changed for e in estimates}
        print("pre-flight (records changed since the watermark):")
        for e in estimates:
            print(f"  {e.endpoint:<11} since {e.since}  ->  {e.changed} to fetch")

    def on_endpoint_start(self, endpoint):
        print(f"== {endpoint}: starting")

    def on_page(self, report, total):
        changed = self.changed.get(report.endpoint, total)
        print("  " + _budget_line(report, changed, self.max_per_hour))

    def on_wait(self, endpoint, seconds):
        print(
            f"  {endpoint}: hourly cap reached, waiting "
            f"{int(seconds)}s for the window to free"
        )

    def finish(self, reports):
        print("\nupdate summary:")
        for r in reports:
            state = _state(r)
            print(
                f"  {r.endpoint:<11} fetched={r.fetched} staged={r.staged} "
                f"pages={r.pages} watermark={r.last_sync} — {state}"
            )

    def close(self):
        pass


class _RichDisplay:
    def __init__(self, endpoints, max_per_hour: int):
        self.max_per_hour = max_per_hour
        self.console = Console()
        # Latest report per endpoint, in run order.
        self.rows = {ep: None for ep in endpoints}
        self.changed = {ep: 0 for ep in endpoints}
        self.waiting = {}
        self.live = Live(self._render(), console=self.console, refresh_per_second=8)
        self.live.start()

    def _render(self):
        table = Table(title="Comic Vine cache update", expand=True)
        table.add_column("endpoint")
        table.add_column("page", justify="right")
        table.add_column("fetched / changed", justify="right")
        table.add_column("%", justify="right")
        table.add_column("staged", justify="right")
        table.add_column("state")
        for ep, report in self.rows.items():
            changed = self.changed.get(ep, 0)
            if report is None:
                total = f"0 / {changed}" if changed else "-"
                table.add_row(ep, "-", total, "-", "-", "pending")
                continue
            pct = f"{100 * report.fetched // changed}%" if changed else "—"
            state = _state(report)
            wait = self.waiting.get(ep)
            if wait:
                state = f"waiting {int(wait)}s (cap)"
            table.add_row(
                ep,
                str(report.pages),
                f"{report.fetched} / {changed}",
                pct,
                str(report.staged),
                state,
            )
        return Panel(table)

    def on_preflight(self, estimates):
        for e in estimates:
            self.changed[e.endpoint] = e.changed
        self.live.update(self._render())

    def on_endpoint_start(self, endpoint):
        self.waiting.pop(endpoint, None)
        self.live.update(self._render())

    def on_page(self, report, total):
        self.waiting.pop(report.endpoint, None)
        self.rows[report.endpoint] = report
        self.live.update(self._render())

    def on_wait(self, endpoint, seconds):
        self.waiting[endpoint] = seconds
        self.live.update(self._render())

    def finish(self, reports):
        for r in reports:
            self.rows[r.endpoint] = r
        self.live.update(self._render())
        self.live.stop()

    def close(self):
        self.live.stop()


def _state(report) -> str:
    if report.complete:
        return "complete"
    if getattr(report, "capped", False):
        return "stopped early (resumable)"
    return "running"
