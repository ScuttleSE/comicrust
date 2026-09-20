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


def _cmd_usage(args) -> int:
    import datetime as _dt

    live = commands.open_v4(Path(args.into))
    try:
        rows = update_mod.usage(live, args.max_per_hour)
    finally:
        live.close()
    if not rows:
        print("no API requests recorded yet.")
        return 0
    print(f"{'resource':<12} {'last hour':>9} {'remaining':>9} "
          f"{'total':>9}  last request")
    for r in rows:
        when = (
            _dt.datetime.fromtimestamp(r.last_request_at).isoformat(" ", "seconds")
            if r.last_request_at is not None else "-"
        )
        print(f"{r.resource:<12} {r.last_hour:>9} {r.remaining:>9} "
              f"{r.total:>9}  {when}")
    return 0


def _parse_deadline(until: str | None, for_minutes: int | None) -> float | None:
    """Turns --until HH:MM (or ISO) / --for N into a Unix deadline. An
    HH:MM already past today rolls to tomorrow, so a job that starts at
    05:00 with --until 04:00 targets tomorrow, not an instant stop."""
    import datetime as _dt

    if for_minutes is not None:
        return time.time() + for_minutes * 60
    if not until:
        return None
    now = _dt.datetime.now()
    try:
        if ":" in until and len(until) <= 5:
            hh, mm = until.split(":")
            target = now.replace(hour=int(hh), minute=int(mm), second=0,
                                 microsecond=0)
            if target <= now:
                target += _dt.timedelta(days=1)
        else:
            target = _dt.datetime.fromisoformat(until)
    except ValueError as exc:
        raise SystemExit(f"bad --until value {until!r}: {exc}")
    return target.timestamp()


def _cmd_rich(args) -> int:
    deadline = _parse_deadline(getattr(args, "until", None),
                               getattr(args, "for_minutes", None))

    def progress(report, rid):
        print(
            f"  {args.mode} {rid:>9}  fetched={report.fetched} "
            f"stored={report.credited} missing={report.skipped_missing} "
            f"remaining={report.remaining}",
            flush=True,
        )

    def on_wait(resource, seconds):
        print(f"  {resource}: rate-limited, waiting {int(seconds)}s",
              flush=True)

    prog = None if args.quiet else progress
    wait = None if args.quiet else on_wait

    if args.mode == "all":
        def on_phase(phase, resource):
            if not args.quiet:
                print(f"== {phase}: {resource}", flush=True)

        rep = update_mod.run_all(
            Path(args.into), api_key=args.api_key,
            deadline=deadline, delay_seconds=args.delay,
            max_per_hour=args.max_per_hour, backfill_on_cap=args.on_cap,
            make_backup=not args.no_backup,
            on_phase=on_phase, on_progress=prog, on_wait=wait,
        )
        fwd = sum(r.credited for r in rep.forward.values())
        bkf = sum(r.credited for r in rep.backfill.values())
        tail = "reached deadline" if rep.reached_deadline else "done"
        print(f"all: forward stored={fwd} backfill stored={bkf} — {tail}")
        return 0

    common = dict(
        delay_seconds=args.delay,
        max_per_hour=args.max_per_hour,
        on_cap=args.on_cap,
        make_backup=not args.no_backup,
        on_progress=prog,
        on_wait=wait,
        deadline=deadline,
    )
    if args.mode == "issues-backfill":
        report = update_mod.rich_issue_backfill(
            Path(args.into), api_key=args.api_key,
            max_pages=args.max_pages, **common,
        )
    elif args.mode.endswith("-backfill"):
        resource = args.mode[: -len("-backfill")]
        report = update_mod.rich_resource_backfill(
            Path(args.into), api_key=args.api_key, resource=resource,
            max_pages=args.max_pages, **common,
        )
    elif args.mode.endswith("-forward"):
        resource = args.mode[: -len("-forward")]
        report = update_mod.rich_resource_forward(
            Path(args.into), api_key=args.api_key, resource=resource,
            since=args.since, max_pages=args.max_pages, **common,
        )
    else:
        print(f"unknown rich mode {args.mode!r}")
        return 2
    state = "stopped early (resumable)" if report.stopped_capped else "done"
    print(
        f"{args.mode}: fetched={report.fetched} stored={report.credited} "
        f"missing={report.skipped_missing} remaining={report.remaining} "
        f"(of {report.total}) — {state}"
    )
    return 0


def _cmd_hashes(args) -> int:
    def progress(report, image_id):
        print(
            f"  image {image_id:>9}  hashed={report.hashed} failed={report.failed}",
            flush=True,
        )

    report = update_mod.hash_backfill(
        Path(args.into),
        max_images=args.max,
        delay_seconds=args.delay,
        all_images=args.all_images,
        make_backup=not args.no_backup,
        on_progress=None if args.quiet else progress,
    )
    state = "stopped early (resumable)" if report.stopped_capped else "done"
    print(f"hashes: hashed={report.hashed} failed={report.failed} — {state}")
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

    p_usage = sub.add_parser(
        "usage", help="show shared API request-log usage per resource"
    )
    p_usage.add_argument("--into", required=True)
    p_usage.add_argument(
        "--max-per-hour",
        type=int,
        default=update_mod.MAX_PER_HOUR,
        help="the per-resource hourly cap used to compute remaining",
    )
    p_usage.set_defaults(func=_cmd_usage)

    p_rich = sub.add_parser(
        "rich", help="fetch rich per-resource detail (credits, images)"
    )
    p_rich.add_argument("--into", required=True)
    p_rich.add_argument("--api-key", required=True)
    p_rich.add_argument(
        "--mode",
        choices=(
            "all",
            "issues-backfill",
            "person-backfill", "character-backfill", "volume-backfill",
            "team-backfill", "location-backfill", "story_arc-backfill",
            "person-forward", "character-forward", "volume-forward",
            "team-forward", "location-forward", "story_arc-forward",
        ),
        default="issues-backfill",
        help="which rich pass to run; 'all' runs forward then backfill "
        "for every resource under one --until deadline",
    )
    p_rich.add_argument(
        "--until",
        help="stop backfilling at this wall-clock time (HH:MM, rolling to "
        "tomorrow if already past, or an ISO timestamp)",
    )
    p_rich.add_argument(
        "--for", dest="for_minutes", type=int,
        help="stop after this many minutes (alternative to --until)",
    )
    p_rich.add_argument(
        "--since",
        help="forward modes only: override the rich_forward watermark",
    )
    p_rich.add_argument(
        "--max-pages",
        type=int,
        help="stop after N issues this run (resumable backfill slice)",
    )
    p_rich.add_argument("--delay", type=float, default=1.0,
                        help="minimum seconds between API calls")
    p_rich.add_argument("--max-per-hour", type=int,
                        default=update_mod.MAX_PER_HOUR)
    p_rich.add_argument("--on-cap", choices=("wait", "stop"), default="wait",
                        help="single-resource modes only; ignored by 'all', "
                             "which cycles resources and sleeps only when all "
                             "are capped")
    p_rich.add_argument("--quiet", action="store_true")
    p_rich.add_argument("--no-backup", action="store_true")
    p_rich.set_defaults(func=_cmd_rich)

    p_hash = sub.add_parser(
        "hashes", help="download covers and fill ComicTagger cover hashes"
    )
    p_hash.add_argument("--into", required=True)
    p_hash.add_argument(
        "--max", type=int,
        help="stop after N images this run (resumable)",
    )
    p_hash.add_argument(
        "--delay", type=float, default=0.3,
        help="seconds between image downloads (CDN politeness; not the "
        "API budget, which images do not use)",
    )
    p_hash.add_argument(
        "--all-images", action="store_true",
        help="hash every gallery image, not just the front cover",
    )
    p_hash.add_argument("--quiet", action="store_true")
    p_hash.add_argument("--no-backup", action="store_true")
    p_hash.set_defaults(func=_cmd_hashes)

    args = parser.parse_args(argv)
    return args.func(args)


if __name__ == "__main__":
    sys.exit(main())
