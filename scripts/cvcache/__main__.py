"""The cvcache script CLI.

    python3 -m scripts.cvcache build   --out FILE MCL [MCL ...]
    python3 -m scripts.cvcache merge   --into FILE MCL [MCL ...]
    python3 -m scripts.cvcache import-localcv --into FILE --source localcv.db
                                   [--no-backup]

The one-off localcv import takes the whole database. The publisher
filter (`--whitelist` / `--blacklist`) is optional and exists for a
later probe-the-CV-API workflow, not for the batch import; leave it off
to import everything.

Run from the repository root so `scripts.cvcache` resolves.
"""

from __future__ import annotations

import argparse
import sys
import time
from pathlib import Path

from . import commands
from .adapters.localcv import LocalCvAdapter
from .publishers import PublisherFilter
from . import update as update_mod
from .progress import make_display


def _print_report(report) -> None:
    print("merge report:")
    for table in report.tables:
        if table.added or table.updated or table.skipped or table.rejected:
            print(
                f"  {table.name:<22} added={table.added} updated={table.updated} "
                f"skipped={table.skipped} rejected={table.rejected}"
            )


def _cmd_build(args) -> int:
    report = commands.build([Path(p) for p in args.mcl], Path(args.out))
    print(
        f"build: volumes={report.volumes} issues={report.issues} "
        f"skipped_lines={report.skipped} -> {args.out}"
    )
    return 0


def _cmd_merge(args) -> int:
    report, backup_path = commands.merge_mcl(
        [Path(p) for p in args.mcl], Path(args.into), make_backup=not args.no_backup
    )
    if backup_path:
        print(f"backup: {backup_path}")
    _print_report(report)
    return 0


def _cmd_import_localcv(args) -> int:
    flt = None
    if args.whitelist or args.blacklist:
        flt = PublisherFilter.from_files(
            Path(args.whitelist) if args.whitelist else None,
            Path(args.blacklist) if args.blacklist else None,
        )
    the_adapter = LocalCvAdapter(
        db_path=Path(args.source),
        fetched_at=int(time.time()),
        publisher_filter=flt,
    )
    outcome, report, backup_path = commands.import_adapter(
        the_adapter, Path(args.into), make_backup=not args.no_backup
    )
    if backup_path:
        print(f"backup: {backup_path}")
    print(f"staged={outcome.staged} rejected={outcome.rejected}")
    for table, reason in outcome.rejects[:10]:
        print(f"  reject {table}: {reason}")
    if the_adapter.dropped:
        for reason, count in sorted(the_adapter.dropped.items()):
            print(f"  dropped {reason}: {count}")
    if the_adapter.numberless_issue_ids:
        shown = the_adapter.numberless_issue_ids
        print(f"  numberless issue ids ({len(shown)}): "
              f"{', '.join(str(i) for i in shown)}")
    _print_report(report)
    return 0


def _cmd_update(args) -> int:
    flt = None
    if args.whitelist or args.blacklist:
        flt = PublisherFilter.from_files(
            Path(args.whitelist) if args.whitelist else None,
            Path(args.blacklist) if args.blacklist else None,
        )
    endpoints = (
        tuple(args.endpoint) if args.endpoint else update_mod.ENDPOINTS
    )
    display = make_display(endpoints, args.max_per_hour, args.quiet)
    report = update_mod.run(
        Path(args.into),
        api_key=args.api_key,
        endpoints=endpoints,
        max_pages=args.max_pages,
        since=args.since,
        delay_seconds=args.delay,
        publisher_filter=flt,
        make_backup=not args.no_backup,
        max_per_hour=args.max_per_hour,
        on_cap=args.on_cap,
        on_page=display.on_page,
        on_wait=display.on_wait,
        on_endpoint_start=display.on_endpoint_start,
        on_preflight=display.on_preflight,
        dry_run=args.dry_run,
    )
    if not args.dry_run:
        display.finish(report.endpoints)
    else:
        display.close()
    return 0


def main(argv=None) -> int:
    parser = argparse.ArgumentParser(prog="cvcache")
    sub = parser.add_subparsers(dest="command", required=True)

    p_build = sub.add_parser("build", help="build a fresh file from MCL")
    p_build.add_argument("--out", required=True)
    p_build.add_argument("mcl", nargs="+")
    p_build.set_defaults(func=_cmd_build)

    p_merge = sub.add_parser("merge", help="merge MCL into a file")
    p_merge.add_argument("--into", required=True)
    p_merge.add_argument("--no-backup", action="store_true")
    p_merge.add_argument("mcl", nargs="+")
    p_merge.set_defaults(func=_cmd_merge)

    p_import = sub.add_parser("import-localcv", help="import a localcv.db")
    p_import.add_argument("--into", required=True)
    p_import.add_argument("--source", required=True)
    p_import.add_argument("--whitelist", help="optional; for future probe use")
    p_import.add_argument("--blacklist", help="optional; for future probe use")
    p_import.add_argument("--no-backup", action="store_true")
    p_import.set_defaults(func=_cmd_import_localcv)

    p_update = sub.add_parser(
        "update", help="pull changed rows from the CV API since the watermark"
    )
    p_update.add_argument("--into", required=True)
    p_update.add_argument("--api-key", required=True)
    p_update.add_argument(
        "--endpoint",
        action="append",
        choices=update_mod.ENDPOINTS,
        help="restrict to one endpoint (repeatable); default is all",
    )
    p_update.add_argument(
        "--max-pages",
        type=int,
        help="stop after N pages per endpoint (a resumable backfill slice)",
    )
    p_update.add_argument(
        "--since",
        help="override the start date (YYYY-MM-DD); default is the watermark",
    )
    p_update.add_argument(
        "--delay",
        type=float,
        default=1.0,
        help="minimum seconds between API calls (rate-limit spacing)",
    )
    p_update.add_argument("--whitelist", help="optional publisher whitelist")
    p_update.add_argument("--blacklist", help="optional publisher blacklist")
    p_update.add_argument("--no-backup", action="store_true")
    p_update.add_argument(
        "--max-per-hour",
        type=int,
        default=update_mod.MAX_PER_HOUR,
        help="per-endpoint hourly request cap (CV allows 200)",
    )
    p_update.add_argument(
        "--on-cap",
        choices=("wait", "stop"),
        default="wait",
        help="at the hourly cap: wait for the window to free, or stop "
        "(resumable). Default wait.",
    )
    p_update.add_argument(
        "--quiet",
        action="store_true",
        help="plain text output instead of the live progress display",
    )
    p_update.add_argument(
        "--dry-run",
        action="store_true",
        help="run only the pre-flight probe: print the changed-row count "
        "per endpoint and exit without fetching",
    )
    p_update.set_defaults(func=_cmd_update)

    args = parser.parse_args(argv)
    return args.func(args)


if __name__ == "__main__":
    sys.exit(main())
