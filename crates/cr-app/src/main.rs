//! Main comicrust binary. Phase 3 wiring: everything after the
//! program name is a comic path to open. D-Bus single instance,
//! packaging, and workspace persistence arrive in Phases 7/8
//! (`docs/archive/phases/phase-3.md` — skip the D-Bus piece now).

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    cr_ui::run(args);
}
