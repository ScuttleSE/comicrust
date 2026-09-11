# AGENTS.md — Agent Onboarding

You work on **comicrust**: a from-scratch port of **ComicRack Community
Edition** (a Windows C# WinForms comic library manager and reader) to a
**Linux-native Rust + GTK4 application** with full 1:1 feature parity.

## Startup sequence

Read these in order. Do not read more than this by default.

1. This file.
2. `docs/current-status.md` — the active phase, the current task, the open
   user tests.
3. The active phase file in `docs/phases/` (the status file names it).
4. The ADRs in `docs/decisions.md` that the phase file links to.
5. The guides in `docs/guides/` that your task needs.
6. The C# source for the behavior you port (see "Reference codebase").

Read `docs/archive/` only when your current task links to a file there.
Archived material is history, not instruction.

## Source of truth

| Question | Answer lives in |
|---|---|
| What must the program do? | The C# source (`/home/scuttle/Downloads/repo/ComicRackCE`) |
| What is decided and locked? | `docs/decisions.md` |
| What is the state right now? | `docs/current-status.md` |
| What work is in flight? | The active file in `docs/phases/` |
| How do I do a recurring job? | `docs/guides/` |
| What is planned later? | `docs/backlog.md`, `docs/port-plan.md` |
| What happened before? | `docs/archive/`, git history |

When a document and the C# source disagree, the C# source wins. When the C#
source and a real ComicRack data file disagree, the data file wins.

---

## Hard rules

These rules are absolute. They exist because breaking them wastes the user's
time and produces false work.

### Rule 0: No invention (THE MOST IMPORTANT RULE)

- Never state a cause, a mechanism, or a fact that you did not read or
  measure. If you do not know, say that you do not know.
- You get a maximum of **two** speculations on any one problem. Write each
  one as "Speculation 1 of 2:" with the evidence that supports it, then ask
  the user. A third speculation does not exist.
- Never act on a speculation without the user's explicit approval.
- Never fill a gap in your knowledge with a plausible guess. Ask for the
  data, or propose adding logging to get it.
- Never chain guesses: do something, invent a cause, do something else,
  invent another cause. That is forbidden even when each step looks small.

### Rule 1: Evidence before claims

- Find the true cause before you name a cause.
- Never clear or blame a change without a measurement.
- Trust the trace over the theory. If `strace`, `gdb`, or a log disagrees
  with your reading of the code, the data wins.
- Never assume the user's environment. The user runs the binary on a
  different machine. Local disk state, tools, and timing do not transfer.
- "It builds" is not evidence that a fix works. Confirm the fix against the
  measured problem.

### Rule 2: Report what you find

- Report important evidence before you act on it. You may group related
  read-only checks into one step.
- Report all three states at every step: what passed, what failed, what is
  unknown.
- A result that contradicts your expectation is a full stop. Present the raw
  evidence and ask. Do not start a guess-and-test cycle.
- A gate that gives different answers on identical code is a reported
  finding, not a problem to redesign on your own.

### Rule 3: No tweak-until-green

- Never change an expected value, a threshold, a timing, a seed, or an
  assertion to make a failing check pass.
- A failing gate goes to the user first, with the evidence.

### Rule 4: No loops

- Do not repeat an action that produces no new information. Before you
  repeat a command, name in writing what new information the repeat gives.
- If the cause is not clear after two or three file reads, stop and ask one
  targeted question.
- The moment you notice a repeated action class (same command, same file,
  same search), stop and ask.

### Rule 5: Short answers

- Give the short answer first.
- When you ask the user to run something, write only the task.
- State the plan once. Do not restate it after each step.

### Rule 6: Plan mode is observe-only

- No edits, no commits, no pushes, and no state changes until the user
  approves. The approved plan is the full scope.

### Rule 7: Commit and push

- Commit all changes after you complete a change, then push. A push starts
  CI.
- Never commit user library data. See `docs/guides/data-safety.md`.

### Rule 8: Language

- Write all communication and documentation in Simplified Technical English
  (ASD-STE100). Use the `asd-ste100` skill for new text and for rewrites.

### Rule 9: Never block the GTK main thread

- Any operation that can take real time (archive I/O, a subprocess, a scan,
  a database pass, an image decode) runs on a worker thread. The pattern and
  its constraints are in `docs/guides/gtk-and-ui.md`.

### Breach response

A breach of any rule above has one permitted response: stop immediately,
make no further tool call, report the breach in one line (which rule, what
happened), and wait for the user. "Let me just finish this step" is a second
breach.

---

## Compatibility invariants

Do not break these.

1. **ComicDb.xml read and write.** Element names, attribute names, casing,
   and structure must match the C# `XmlSerializer` output exactly. Golden
   round-trip tests verify this. The database is the one artifact users
   cannot lose.
2. **Metadata schema compatibility.** `ComicInfo.xml` (Anansi standard),
   ComicRack's `ComicBook.xml`, and `MetronInfo.xml` are read and written
   in-archive.
3. **Smart-list query language.** Saved queries must parse and match
   identically. Queries that carry `Expression` or plugin matchers parse and
   render byte-stably, then evaluate to an explicit not-supported result
   (ADR-027: there is no scripting host).
4. **Caches are disposable. The database is not.** Thumbnail and image cache
   formats have no compatibility requirement.

## Known constraints

- **unrar is GPL-incompatible.** Use a subprocess or libarchive. Never
  static-link unrar.
- **No scripting host** (ADR-027). The IronPython plugin ecosystem is not
  ported.
- **The WCF net.tcp remote protocol is not preserved.** Android app protocol
  compatibility was dropped.
- **NTFS ADS metadata maps to Linux xattrs** (`user.comicrack.*`) with a
  sidecar fallback.
- **Reflection by property name is load-bearing in the C#** (matchers,
  columns, remote updates, options panels). The port uses an explicit
  property registry in `cr-core`.
- **Windows paths are baked into user data.** Be lenient when you load.
- **Localization is data-driven per widget name.** Port the `TR` lookup. Do
  not convert it to gettext.

---

## Crate layout

| Crate | Contents |
|---|---|
| `crates/cr-core` | Data model, ComicDb.xml serde, settings, filename parsing |
| `crates/cr-io` | Comic providers, metadata read and write-back, archives |
| `crates/cr-image` | Image currency type, decode and encode, resize, caches |
| `crates/cr-engine` | Matchers, smart lists, queues, scanner, watch folders |
| `crates/cr-scrape` | The Comic Vine scraper module (ADR-031) |
| `crates/cr-ui` | GTK4 reader, browser, shell, dialogs, theming |
| `crates/cr-cli` | Headless verification tooling |
| `crates/cr-app` | Main binary and app wiring |

## Verification

The required command sequence and the environment gates are in
`docs/guides/verification.md`. The short form:

```sh
cargo fmt --all
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

UI behavior is decided by a user test, never by a passing build.

## Reference codebase (the specification)

- **Local checkout:** `/home/scuttle/Downloads/repo/ComicRackCE`
- **Upstream:** https://github.com/maforget/ComicRackCE (branch `master`)

Find and read the C# code before you implement any behavior. Never guess
from names, screenshots, or memory. The reference is decompiled: expect dead
code, odd names, and swallowed exceptions. The target is behavior, not
style. The project map is in `docs/port-plan.md`.

## Conventions

- Commits use the imperative mood with a concise subject.
- Add decisions to `docs/decisions.md` as new ADRs. That file is append-only.
  Language-only rewrites are allowed.
- Update `docs/current-status.md` at the end of every session.
- Keep documents inside their responsibility. The rules are in
  `docs/guides/documentation.md`.
