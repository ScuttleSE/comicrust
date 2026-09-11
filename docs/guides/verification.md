# Guide: verification

This guide holds the verification procedure. The current pass or fail record
lives in `docs/current-status.md`.

## Required sequence

Run these three before every commit. All three must be green.

```sh
cargo fmt --all
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

## Headless verification tools

`cr-cli` checks behavior against real data without a UI.

```sh
cargo run -p cr-cli -- db-roundtrip <ComicDb.xml>
cargo run -p cr-cli -- db-dump <ComicDb.xml>
cargo run -p cr-cli -- lists <ComicDb.xml>
cargo run -p cr-cli -- info <comic-file>
cargo run -p cr-cli -- pages <comic-file>
cargo run -p cr-cli -- extract <comic-file> <page> -o <out>
```

The `pages` JSON is the ground truth when a test must know which entry is
page N. It uses a natural sort of the full entry names. An archive without a
`00000` cover shifts by one.

## Environment gates

Some suites need an environment variable or an external tool.

| Gate | Command | Needs |
|---|---|---|
| Subprocess formats | `CR_FORMAT_TESTS=1 cargo test -p cr-io` | `7z` on `PATH` |
| PDF | the same suite | `CR_PDFIUM=<path to libpdfium.so>` |
| DjVu | the same suite | `c44`, `djvm`, `ddjvu` on `PATH` |

## The golden snapshot

Re-bless the `db-large.xml` snapshot only after a deliberate model change:

```sh
CR_BLESS=1 cargo test -p cr-core --test golden_roundtrip
```

Re-blessing changes fixture bytes. Review the diff before you commit it.

## UI probes

- Run the app in RELEASE mode. A debug build decodes about 50 times slower
  (1.9 s against 34 ms per page).
- Start `Xvfb :99`, then set `GDK_BACKEND=x11` and `DISPLAY=:99`.
- Take screenshots with ImageMagick `import -window root`.
- Send keys with `xdotool key <key>` and NO `--window` flag. Call
  `xdotool windowfocus <wid>` first. The `key --window` form uses
  XSendEvent and reaches nothing.
- Expect the first key press to drop before the toplevel is active.
- `xdotool click 4/5` does NOT produce scroll events under GTK and X11. The
  wheel path is user-testable only.
- When keys stay dead for every binary, suspect the probe first. Bisect with
  `git stash` before you blame the code.

Committed UI tests are the pure-geometry suites only. Screenshots decide
rendering. User tests decide input behavior.

## CI

CI runs on Gitea Actions, on the `docker-runner-amd64` runner, inside the
`comicrust-ci` container image (`.gitea/container/Dockerfile`).

- `ci.yaml` runs fmt, clippy, and tests on every push to main, with
  `CR_FORMAT_TESTS=1`.
- `release.yaml` builds `cr-app` in release mode and republishes the single
  `rolling` prerelease.
- `tagged-release.yaml` runs on manual dispatch with a tag input. Create the
  tag first.

Both release workflows publish through `.gitea/publish_release.sh`. See
ADR-020 and ADR-021.

## The final gate

A passing build is not proof that a UI change works. UI behavior is decided
by a user test. Record open user tests in `docs/current-status.md`.

## When a gate is unstable

A gate that gives different answers on identical code is a reported finding.
Report it with the evidence. Do not redesign the gate alone. Do not change a
threshold, a timing, a seed, or an expected value to make it pass.
