# comicrust

A from-scratch port of [ComicRack Community Edition](https://github.com/maforget/ComicRackCE) — the legendary Windows comic library manager and reader — to a **Linux-native Rust + GTK4 application**, aiming for **full 1:1 feature parity**.

## Status

**Planning phase.** Feasibility analysis and the port plan are complete; implementation has not started. See `docs/port-plan.md` for the roadmap and `AGENTS.md` for current status.

## Why

ComicRack is a legendary comic manager abandoned by its author 10+ years ago, revived as a decompiled Community Edition for Windows. Linux users currently rely on Wine. This project rebuilds it natively: Rust for the engine (memory safety, performance, packaging) and GTK4 for the UI, while staying **byte-compatible with existing ComicRack libraries** (`ComicDb.xml`) and the **Python plugin ecosystem**.

## Scope highlights

- Read/write existing ComicRack libraries (`ComicDb.xml`) with golden-file-verified fidelity
- All comic formats: CBZ/CBR/CB7/CBT, PDF, DjVu, folder/web sources
- Page formats: JPEG/PNG/GIF/TIFF/WebP/HEIF/AVIF/JPEG XL/JPEG2000
- The full browser (thumbnail/tile/detail views, grouping, stacking) and the reader (single/double/continuous, zoom/pan/rotate, transitions, paper textures)
- Python plugin compat via PyO3 (`.py` scripts with `#@Directive` headers, `.crplugin` packages)
- 19 existing translation files reused as-is
- Explicitly dropped: WCF/Android remote protocol compat (a new HTTP API may come later)

## Documentation

| Doc | Contents |
|---|---|
| [AGENTS.md](AGENTS.md) | Agent onboarding: spec location, source map, invariants, gotchas, status tracker |
| [docs/feasibility.md](docs/feasibility.md) | Full feasibility analysis of the ~193k-LOC C# codebase |
| [docs/port-plan.md](docs/port-plan.md) | Crate architecture, C#→Rust technology mapping, 9-phase roadmap |
| [docs/decisions.md](docs/decisions.md) | Architecture decision records (ADR) |
| [docs/risk-register.md](docs/risk-register.md) | Top risks with mitigations |
| [docs/phase-0-kickoff.md](docs/phase-0-kickoff.md) | Concrete first-phase task breakdown |

## Attribution

ComicRack was created by Markus Eisenstöck (cYo) and revived as Community Edition by [maforget](https://github.com/maforget) and contributors. This port uses the CE source as its behavioral specification; all credit for the original design belongs there.

## License

Not yet chosen — deferred intentionally (see `docs/decisions.md`, ADR-009). The upstream CE lineage and third-party licensing constraints (unrar, plugin ecosystem) make this a decision that needs care.
