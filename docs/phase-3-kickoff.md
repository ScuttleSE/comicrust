# Phase 3 Kickoff — Reader UI (GTK4 shell + renderer)

Goal: comicrust opens a comic file in a GTK4 window and renders it comfortably — the daily-driver reading session. This is the first phase with a UI. Everything from Phases 0-2 stays headless and must not regress.

Read first: `AGENTS.md` (rules), `docs/port-plan.md` (architecture, tech mapping), `docs/decisions.md` (ADR-004: no libadwaita; ADR-008: cairo first, GL later). Phase 2's kickoff records the engine entry points you will call.

## Status (2026-09-03) — T1-T3 COMPLETE, T4-T6 REMAIN

Done and user-tested (each task ended with a manual user test on the
user's machine — keep that loop for T4-T6):

- **T1.** `cr-ui` app shell (`app.rs`, `theme.rs`), reader window,
  `cr-app` wiring. Opens a comic from the command line
  (GApplication `open` signal) or the file chooser. GTK 4.0-era API
  only (see ADR-018).
- **T2.** `reader/display.rs` — the `ImageDisplayControl`
  `DisplayOutput`/`DisplayOutputConfig` port as pure, unit-tested
  geometry (fit modes with anamorphic tolerance, part grid,
  binding-edge logic, RTL, rotation, clamped offsets, interpolate).
  `reader/page_view.rs` — the page widget: cairo draw through the
  part transform, background decode worker (latest-wins mailbox +
  `timeout_add_local` pump), logical-page-ahead navigation, zoom
  around the inverse-transformed point, pan with drag deltas.
- **T3.** The `ComicDisplayControl` layer in the same widget:
  `reader/continuous.rs` (`ContinuousPageLayout` port), spread
  composition (`compose_spread`: cover-right rule, RTL swap,
  `DoublePageOverlap` trim, forced-double slot), layout modes
  Single/Double/DoubleAdaptive/Continuous, Fade/LeftRight/TopDown
  transitions (Paging degrades to Fade until GL), paper texture
  (bundled `cr-ui/assets/papers`), Auto/Color/Texture backgrounds.
  Unit tests for the geometry: part grid, spread rules, anchors,
  continuous visibility.

Remaining Phase 3 tasks (start at T4; read the lessons in
`AGENTS.md` first):

- **T4.** Input: port the real `MainForm` reader accelerators and
  mouse map (the current keys 1/2/3/4, P, F, R, +/-, arrows are
  documented test hooks, not the C# map).
- **T5.** Fullscreen + overlay chrome, reader tabs/undocked windows,
  reading-state write-back (`CurrentPage`/`LastPageRead`/
  `OpenedTime`/`OpenedCount` into ComicBook; `last_read` already
  tracks in the widget).
- **T6.** Magnifier (cairo second-draw pass acceptable), error page +
  error thumbnail, page pre-caching through the Phase 2 `ImagePool`
  queues (replace the per-widget latest-wins worker with the real
  fast/slow queues when it lands).

Architecture notes for T4-T6 (ADR-017): one widget (`PageView`)
renders one virtual image through the part machinery; the comic layer
composes pages into that virtual image (single, spread, strip).
Continuous mode keeps the whole scroll in part 0's offset — do not
reintroduce part-index stepping there.

## What exists when you start

| Need | Where |
|---|---|
| Book model, `ComicBook` defaults, id | `cr-core` (`model/comic_book.rs`) |
| Page decode → RGBA `Image`, EXIF-strip quirk | `cr-image` (`decode.rs`, `normalize_to_jpeg`) |
| Adjust/rotate/resize, thumbnails | `cr-image` (`adjust.rs`, `lib.rs rotate`, `resize.rs`, `thumbnail.rs`) |
| Open providers per format, page bytes | `cr-io` (`ComicProvider::open`, `read_page`) |
| Page/thumbnail memory + disk caches, five render queues | `cr-engine` (`image_pool.rs`, `queue.rs`) |
| Reading state (current page, last page read, page count) | `cr-core` ComicBook fields |
| Headless CLI harness | `cr-cli` (`info`, `pages`, `extract`, `thumb`, ...) |

## Scope from the roadmap

GTK4 shell skeleton, GL renderer port: single/double/adaptive/continuous layouts, fit modes, zoom/pan/rotation, transitions, magnifier, paper texture, gestures, fullscreen/undock, tabs. Exit gate: **a comfortable daily-driver reading session**.

## The strategy decision that shapes everything (ADR-008)

Render with **cairo first** (GDK-paintable / GtkSnapshot + cairo), keep the renderer behind a trait so the `GtkGLArea`+`glow` path (GL 3.2 core) can slot in later for transitions, magnifier, and large-image performance. This mirrors the C# architecture (Tao OpenGL with a GDI+ fallback). Do NOT start with GL.

## Suggested task breakdown

### T1. `cr-ui` skeleton + `cr-app` wiring

- [ ] `cr-ui`: app class, main window, menubar-less header bar; `cr-app`: main binary wiring (phases 7 adds D-Bus single instance — skip now).
- [ ] GTK CSS theming skeleton (dark-mode-friendly, no libadwaita).
- [ ] Open-a-file dialog → `ComicProvider::open` → first page on screen via cairo. This alone is the "walking skeleton" — get it green before anything else.
- [ ] Verify: `cargo run -p cr-app -- <file.cbz>` shows a page.

### T2. Reader widget: `ImageDisplayControl` port

The C# spec: `ComicRack.Engine.Display.Forms/ImageDisplayControl.cs` (2,632 LOC).

- [ ] Renderer abstraction trait (cairo implementation first).
- [ ] Layout modes: single page, double page, adaptive (per the C# `ImageLayout` logic).
- [ ] Fit modes (`ImageFit`: width, height, best-fit, original, fullscreen width/height), zoom + pan (scroll/kinetic).
- [ ] Rotation (90/180/270) applied through the page-key pipeline (cr-engine `image_pool` already applies key rotation).
- [ ] Right-to-left / manga reading order.

### T3. `ComicDisplayControl` port (the comic shell around pages)

C# spec: `ComicDisplayControl.cs` (3,514 LOC) and `Engine/Display/ComicDisplay.cs` (2,018 LOC).

- [ ] Continuous scroll mode.
- [ ] Page transitions (paper flip fade/slide; the GL renderer may defer the fancy ones).
- [ ] Paper texture background (`ComicRack/Output/Resources/Textures/Papers`).
- [ ] Dual-page binding edge logic, blank-page insertion on wide spreads.

### T4. Input: keyboard, gestures, mouse

- [ ] Keyboard map matching the C# `MainForm` reader accelerators (arrow/pgup/dn, home/end, R rotate, +/- zoom, 1-6 fit modes...). Copy the bindings from `MainForm.cs` and the localization keys — do not invent new ones.
- [ ] Mouse: wheel = page or scroll, drag = pan, double-click = fit toggle, middle-drag = kinetic scroll.
- [ ] Gestures (GTK4 gesture controllers): pinch zoom, two-finger pan.

### T5. Fullscreen, tabs, undocked reader

- [ ] Fullscreen + overlay chrome auto-hide (C# `AeroFullScreen` handling is Windows-only; port the behavior, not the workaround).
- [ ] Multiple reader tabs (the C# opens comics in the main browser view or undocked reader windows; decide tab shell per `MainForm`).
- [ ] Reading state: store `CurrentPage`/`LastPageRead`/`OpenedTime`/`OpenedCount` back into the ComicBook (cr-core) on navigation.

### T6. Reader polish

- [ ] Magnifier (GL later; cairo magnifier = second scaled draw — acceptable first pass).
- [ ] Error page + error thumbnail (the C# `CreateErrorPage`/`CreateErrorThumbnail`; cr-image can render them).
- [ ] Page pre-caching through the Phase 2 `ImagePool` queues (fast/slow page queues with AddToTop already implement the C# semantics).

## Non-goals for Phase 3

- The browser/library view (Phase 4, `ItemView`).
- Dialogs beyond the file chooser (Phase 5).
- Scripting hooks, remote, sync (Phases 6-7).
- GL renderer if cairo feels good — ADR-008 lets you ship without it.

## Test strategy

- UI tests stay thin: widget behavior tests via `gtk4-rs` test helpers are possible but limited; prefer extracting geometry/layout decisions (fit-mode math, layout page assignment, binding edges) into pure functions in cr-ui/cr-engine and unit-test those (the C# keeps this logic in `ImageDisplayControl` methods you can port mechanically).
- Manual verification is the exit gate: a reading session with real libraries.
- Keep `cargo test --workspace` green; do not let UI crates break the headless CI. Gated CI: the runner has GTK4 dev libs (see `.gitea/workflows/ci.yaml`); render-dependent tests should skip when `DISPLAY`/`WAYLAND_SOCKET` are absent (check `gdk4::is_wayland`/env).

## Risks / lessons from earlier phases that apply

- The C# decompiled source is the spec — port behaviors, not names (see ADR-013 for how quirks are treated).
- The decode/adjust/rotate chain and the EXIF-strip quirk are already proven; do not reimplement them in the UI crate.
- The Phase 2 queues implement AddToTop/bounded sizes; route page fetches through `ImagePool::add_page_with_render` instead of synchronous loads where the C# uses the queues.
- Update the **Current status** section of `AGENTS.md` at the end of every session, and commit+push per task (working rules).
