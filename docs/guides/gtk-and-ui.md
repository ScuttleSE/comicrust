# Guide: GTK and UI

This guide holds the reusable UI rules. Phase-specific rendering detail
stays in the archived phase files under `docs/archive/phases/`.

## Never block the main thread

Any operation that can take real time runs on a worker thread: archive I/O,
a subprocess, a scan, a database pass, an image decode.

The pattern:

1. The main thread starts the work on a worker.
2. The worker sends its result over a std `mpsc` channel.
3. A `timeout_add_local` poll on the main thread drains the channel.

glib 0.22 has NO `MainContext::channel` and no `glib::Sender`. Use the std
`mpsc` bridge.

`ProcessingQueue` callbacks must be `Fn + Send + Sync`. A std
`mpsc::Sender` is not `Sync`, so wrap it, for example
`PageTx(Arc<Mutex<Sender>>)`.

A completion payload must carry an identity (for example the comic source
string) so a stale result from a previous open is dropped.

## RefCell borrows (the most frequent crash class)

Edition 2021 holds the temporaries of an `if` condition until the END of the
whole if/else statement.

```rust
// PANICS: "RefCell already borrowed" when the body borrows again
if self.state.borrow().x == y { self.state.borrow_mut().z = 1; }

// CORRECT: hoist the borrow into a let statement
let v = self.state.borrow().x;
if v == y { self.state.borrow_mut().z = 1; }
```

A borrow hides anywhere in the condition or the scrutinee EXPRESSION, not
only as the direct condition. This shape crashes too:

```rust
// PANICS: the borrow lives through the branch
if let Some(g) = hit_group_header(&state.borrow().layout, ...) { ... }

// CORRECT
let group_hit = { let s = state.borrow(); hit_group_header(&s.layout, ...) };
```

The same rule applies to a `while let` scrutinee: bind the `try_recv()`
result BEFORE you match it.

Sweep for the direct shape with `rg "if (self|view)\.state\.borrow"`. The
sweep MISSES the scrutinee-argument shape. Check call arguments by hand.

This class caused the Phase 3 wheel crash, the 2026-09-10 group
double-click crash, and the 2026-09-11 double-click open crash.

## Initialization and arguments

- `gtk::init()` must run before any object construction. A GTK app aborts
  with "GTK has not been initialized" when `theme::init` runs first.
- Stay on the GTK 4.0-era API surface with gtk4-rs 0.11 (ADR-018).
- GApplication intercepts positional file arguments. Register `HANDLES_OPEN`
  and connect the `open` signal. Do not parse argv for file paths.

## Focus

GTK4 has NO click-to-focus, and `grab_focus` is ignored while the toplevel
is inactive. A widget that must take keys on window activation needs a
re-grab on the window's `is-active` notify. The reader does this for the
main window and for the undocked window.

## Drawing

- A cairo `ImageSurface` built from RGBA needs ARGB pre-multiplication and
  the surface's own stride, not `width * 4`.
- Cairo surfaces are not `Send` or `Sync`. Cache them in a `thread_local`,
  never in a `OnceLock`.

## Build mode

All user tests and UI probes run a release build:

```sh
cargo run -p cr-app --release --
```

A debug build decodes about 50 times slower.

## Probes

The probe rules are in `docs/guides/verification.md`. The short form: key
injection is the fragile part, and only a user test decides input behavior.
