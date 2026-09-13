//! The shell command registry — the port of the `MainForm` command
//! layer (`CommandHandler.cs`/`CommandMapper.cs` + the menu
//! `ShortcutKeys` table in `MainForm.Designer.cs`).
//!
//! Every browser/shell command is a `gio::SimpleAction` in the
//! `win.` group (`CommandMapper.Add` parity: one command, many
//! senders — here the senders are menus, accelerators, and later
//! toolbars). Accelerators are the C# menu-item `ShortcutKeys`,
//! registered per action (detailed names for radio targets).
//!
//! Binding policy: only accelerator combinations WITH modifiers or
//! function keys land here — the reader's own key table
//! (`reader/keys.rs`) owns the plain keys (D1, N, S, ...), exactly
//! like the C# splits `ComicDisplay.KeyboardMap` from the menu
//! shortcuts.
//!
//! Deviations from the C# table (recorded):
//! - Rotate 270 (Ctrl+Shift+D0) is unbound: the C# gives the SAME
//!   shortcut to "Only fit if oversized" and "Rotate 270" (menu
//!   order decides the winner — Page Layout comes first). The port
//!   keeps the binding on Only fit if oversized.
//! - The context-menu-only variants (cmPrevFromList Alt+Shift+P,
//!   cmOnlyFitOversized Ctrl+Alt+O, ...) are context-menu
//!   conveniences; they land with the menus that host them.

/// One shell command: the action name (in the `win.` group) and the
/// C# menu accelerators. `accels` is empty for unbound commands
/// (the C# registers menu items without ShortcutKeys).
#[derive(Debug, PartialEq, Eq)]
pub struct CommandSpec {
    pub action: &'static str,
    pub accels: &'static [&'static str],
}

const fn cmd(action: &'static str, accels: &'static [&'static str]) -> CommandSpec {
    CommandSpec { action, accels }
}

/// The shell commands with their accelerators (`MainForm.Designer.cs`
/// menu items; toolbar/context twins resolve to the same action).
pub const COMMANDS: &[CommandSpec] = &[
    // File
    cmd("open-file", &["<Control>o"]),
    cmd("close", &["<Control>x"]),
    cmd("close-all", &["<Control><Shift>x"]),
    cmd("new-tab", &["<Control>t"]),
    cmd("add-folder", &["<Control><Shift>a"]),
    cmd("scan-folders", &["<Control><Shift>s"]),
    // NO C# item: the Windows-path migration helper (Phase 8 T11,
    // the user request) — a port addition, no accelerator.
    cmd("migrate-paths", &[]),
    cmd("update-book-files", &["<Control><Shift>u"]),
    // The C# binds NO menu shortcut (Ctrl+Shift+T is Tasks); the
    // `Generate Cover Thumbnails` item is a stub until the thumbnail
    // queue work lands.
    cmd("generate-thumbnails", &[]),
    cmd("tasks", &["<Control><Shift>t"]),
    // NO C# item — the Comic Vine disk cache (ADR-037, ADR-038,
    // Phase 15). The C# plugin had no cache, so it had no commands.
    cmd("cv-import-mcl", &[]),
    cmd("cv-update", &[]),
    cmd("cv-warm", &[]),
    cmd("new-book-entry", &["<Control><Shift>n"]),
    // The NewComics.py port (ADR-027) — the script item carried no
    // accelerator.
    cmd("new-book-series", &[]),
    cmd("restart", &["<Control><Shift>q"]),
    cmd("quit", &["<Control>q"]),
    // Edit
    cmd("info", &["<Control>i"]),
    cmd("rating-0", &["<Alt><Shift>0"]),
    cmd("rating-1", &["<Alt><Shift>1"]),
    cmd("rating-2", &["<Alt><Shift>2"]),
    cmd("rating-3", &["<Alt><Shift>3"]),
    cmd("rating-4", &["<Alt><Shift>4"]),
    cmd("rating-5", &["<Alt><Shift>5"]),
    cmd("quick-rating", &["<Alt><Shift>q"]),
    cmd("set-bookmark", &["<Control><Shift>b"]),
    cmd("remove-bookmark", &["<Control><Shift>d"]),
    cmd("prev-bookmark", &["<Control><Shift>p"]),
    // The C# gives Ctrl+Shift+N to BOTH "New fileless Book Entry"
    // (File menu) and "Next Bookmark" (Edit menu); WinForms fires
    // the first match in menu order — File. The port keeps the
    // binding on the File item (same rule as the Rotate 270 case).
    cmd("next-bookmark", &[]),
    cmd("last-page-read", &["<Control><Shift>l"]),
    cmd("copy-page", &["<Control>c"]),
    cmd("export-page", &["<Control><Shift>c"]),
    cmd("refresh", &["F5"]),
    cmd("display-settings", &["F9"]),
    cmd("preferences", &["<Control>F9"]),
    // Browse
    cmd("toggle-browser", &["F3"]),
    // The browser toolbar's Views drop carries it (`miExpandAllGroups`,
    // enabled iff groups are visible); the C# binds no accelerator.
    cmd("toggle-groups", &[]),
    // NO C# item — the duplicate cleanup (ADR-044, port addition):
    // the book context menu hosts it, the C# binds no accelerator.
    cmd("select-worst-duplicates", &[]),
    cmd("view-library", &["F6"]),
    cmd("view-pages", &["F8"]),
    cmd("sidebar", &["<Shift>F6"]),
    cmd("small-preview", &["<Shift>F7"]),
    // NO C# item: the C# theme is a boot switch (`-dark` / the
    // `Theme` ini key, `ExtendedSettings.cs` browsable:false) with no
    // menu command. The port adds the Browse-menu toggle so the mode
    // switches at runtime — recorded deviation.
    cmd("dark-mode", &[]),
    cmd("prev-list", &["<Control>j"]),
    cmd("next-list", &["<Control>k"]),
    // Read
    cmd("first-page", &["<Control>b"]),
    cmd("prev-page", &["<Control>p"]),
    cmd("next-page", &["<Control>n"]),
    cmd("last-page", &["<Control>e"]),
    cmd("prev-book", &["<Control><Alt>p"]),
    cmd("next-book", &["<Control><Alt>n"]),
    cmd("random-book", &["<Control><Alt>o"]),
    cmd("show-in-browser", &["<Control>F3"]),
    cmd("prev-tab", &["<Control><Shift>j"]),
    cmd("next-tab", &["<Control><Shift>k"]),
    cmd("auto-scroll", &["<Control>s"]),
    cmd("double-auto-scroll", &["<Alt><Shift>s"]),
    cmd("track-current-page", &["<Alt><Shift>t"]),
    // Display — the fit modes carry their per-value accels through
    // the detailed names ("win.page-fit::<value>"); the stateful
    // radio action itself is bound separately in the shell.
    cmd("page-fit", &[]),
    cmd("page-layout", &[]),
    cmd("right-to-left", &["<Control>0"]),
    cmd("only-fit-oversized", &["<Control><Shift>0"]),
    // Zoom In is Ctrl+Oemplus in the C# — the '+' key. Its unshifted
    // symbol is '=' (keyval "equal"), the numpad '+' IS keyval
    // "plus"; both spellings register (the shifted '+' key belongs
    // to Rotate Right: Ctrl+Shift+plus).
    cmd("zoom-in", &["<Control>equal", "<Control>plus"]),
    cmd("zoom-out", &["<Control>minus"]),
    cmd("toggle-zoom", &["<Control><Alt>z"]),
    // The Zoom preset items (100..400 %) carry no C# accelerators;
    // the parameter is the percent ("win.zoom-preset::100").
    cmd("zoom-preset", &[]),
    cmd("zoom-custom", &["<Control><Shift>z"]),
    cmd("rotate-left", &["<Control><Shift>minus"]),
    cmd("rotate-right", &["<Control><Shift>plus"]),
    cmd("rotate-0", &["<Control><Shift>7"]),
    cmd("rotate-90", &["<Control><Shift>8"]),
    cmd("rotate-180", &["<Control><Shift>9"]),
    // The C# table gives Ctrl+Shift+D0 to BOTH Only fit if oversized
    // and Rotate 270; the port keeps it on the first (see above).
    cmd("rotate-270", &[]),
    cmd("auto-rotate", &[]),
    cmd("minimal-gui", &["F10"]),
    cmd("full-screen", &["F11"]),
    cmd("undock-reader", &["F12"]),
    cmd("magnifier", &["<Control>m"]),
    // Help
    cmd("about", &["<Alt>F1"]),
    // The `mainKeys` shell commands (MainForm.InitializeKeyboard).
    cmd("focus-search", &["<Control>f"]),
    cmd("toggle-navigator-search", &["<Control><Alt>f"]),
];

/// The `win.page-fit` radio values with the C# Page Layout submenu
/// accelerators (`miOriginal..miBestFit`; Ctrl+D1..Ctrl+D6 = Ctrl+1..6).
pub const FIT_MODES: &[(&str, &str)] = &[
    ("original", "<Control>1"),
    ("fit-all", "<Control>2"),
    ("fit-width", "<Control>3"),
    ("fit-width-adaptive", "<Control>4"),
    ("fit-height", "<Control>5"),
    ("fit-best", "<Control>6"),
];

/// The `win.page-layout` radio values (`miSinglePage..miContinuous`;
/// Ctrl+D7..Ctrl+D9 = Ctrl+7..9; Continuous has no C# accelerator).
pub const LAYOUT_MODES: &[(&str, &str)] = &[
    ("single", "<Control>7"),
    ("double", "<Control>8"),
    ("double-adaptive", "<Control>9"),
    ("continuous", ""),
];

/// The shifted-symbol accelerator fallback (`MainForm` matched the
/// WinForms VIRTUAL keys — `Keys.D4` — which are layout-independent;
/// GTK accelerators match the PRODUCED keyval, so Alt+Shift+4
/// produces '¤'/'$' on most layouts and never matches
/// `<Alt><Shift>4`). Given the modifiers and the UNSHIFTED keyval
/// (resolved from the hardware keycode through
/// `gdk_display_map_keycode`, level 0), returns the command the C#
/// table binds. The caller fires it only when the raw event keyval
/// DIFFERS from the unshifted one — layouts where Shift keeps the
/// symbol let the real accelerator match, so the pair never
/// double-fires.
pub fn shifted_symbol_command(
    ctrl: bool,
    shift: bool,
    alt: bool,
    unshifted: gtk4::gdk::Key,
) -> Option<&'static str> {
    use gtk4::gdk::Key;
    match (ctrl, shift, alt) {
        // Alt+Shift+0..5 — the My Rating items.
        (false, true, true) => match unshifted {
            Key::_0 => Some("rating-0"),
            Key::_1 => Some("rating-1"),
            Key::_2 => Some("rating-2"),
            Key::_3 => Some("rating-3"),
            Key::_4 => Some("rating-4"),
            Key::_5 => Some("rating-5"),
            _ => None,
        },
        // Ctrl+Shift+D7/D8/D9/D0 — Rotate 0/90/180 and Only fit if
        // oversized; Ctrl+Shift+OemMinus — Rotate Left. (The
        // Ctrl+Shift+plus spelling of Rotate Right needs no
        // fallback: '+' is the SHIFTED symbol of the key, the accel
        // matches it directly.)
        (true, true, false) => match unshifted {
            Key::_0 => Some("only-fit-oversized"),
            Key::_7 => Some("rotate-0"),
            Key::_8 => Some("rotate-90"),
            Key::_9 => Some("rotate-180"),
            Key::minus => Some("rotate-left"),
            _ => None,
        },
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn action_names_are_unique() {
        let mut names: Vec<&str> = COMMANDS.iter().map(|c| c.action).collect();
        names.sort_unstable();
        let n = names.len();
        names.dedup();
        assert_eq!(names.len(), n);
    }

    #[test]
    fn accelerators_are_unique_per_action() {
        // The same accel must not appear twice across the whole
        // registered set. The C# table has two collisions, resolved
        // by menu order (File before Edit, Page Layout before
        // Rotation): Only fit if oversized over Rotate 270, and New
        // fileless Book Entry over Next Bookmark. The loser bindings
        // are dropped (see the table).
        let mut seen: HashSet<&str> = HashSet::new();
        for command in COMMANDS {
            for accel in command.accels {
                assert!(
                    seen.insert(accel),
                    "accel {accel} registered twice ({})",
                    command.action
                );
            }
        }
        for (value, accel) in FIT_MODES.iter().chain(LAYOUT_MODES.iter()) {
            if accel.is_empty() {
                continue;
            }
            assert!(
                seen.insert(accel),
                "accel {accel} registered twice (page {value})"
            );
        }
    }

    /// The accel syntax check (pure — `gtk_accelerator_parse` needs
    /// a display, so the test validates the shape itself): modifier
    /// tags from the C# set and a key name from the ones the table
    /// uses.
    fn accel_syntax_ok(accel: &str) -> bool {
        let mut rest = accel;
        while let Some(open) = rest.find('<') {
            let Some(close) = rest[open..].find('>') else {
                return false;
            };
            let tag = &rest[open + 1..open + close];
            if !matches!(tag, "Control" | "Shift" | "Alt") {
                return false;
            }
            rest = &rest[open + close + 1..];
        }
        let key = rest;
        !key.is_empty()
            && (key == "plus"
                || key == "minus"
                || (key.starts_with('F') && key[1..].parse::<u8>().is_ok() && key.len() <= 3)
                || key
                    .chars()
                    .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit()))
    }

    #[test]
    fn every_accel_has_valid_syntax() {
        for command in COMMANDS {
            for accel in command.accels {
                assert!(accel_syntax_ok(accel), "{accel} is malformed");
            }
        }
        for (_, accel) in FIT_MODES.iter().chain(LAYOUT_MODES.iter()) {
            if accel.is_empty() {
                continue;
            }
            assert!(accel_syntax_ok(accel), "{accel} is malformed");
        }
    }

    #[test]
    fn reader_commands_have_no_plain_key_accels() {
        // Plain keys stay with the reader key table — a shell accel
        // without modifiers would swallow text entry (the search box
        // types letters). Function keys are the exception.
        for command in COMMANDS {
            for accel in command.accels {
                let bare = !accel.contains('<');
                let is_fn_key =
                    accel.starts_with('F') && accel[1..].chars().all(|c| c.is_ascii_digit());
                assert!(
                    !bare || is_fn_key,
                    "{}: {accel} has no modifiers — it belongs to the reader key table",
                    command.action
                );
            }
        }
    }

    #[test]
    fn shifted_symbol_fallback_maps_the_csharp_virtual_keys() {
        use gtk4::gdk::Key;
        // Alt+Shift+4 rates 4 stars whatever symbol Shift produces.
        assert_eq!(
            shifted_symbol_command(false, true, true, Key::_4),
            Some("rating-4")
        );
        assert_eq!(
            shifted_symbol_command(false, true, true, Key::_0),
            Some("rating-0")
        );
        // Ctrl+Shift+D7/D8/D9/D0 — Rotate 0/90/180 + Only fit.
        assert_eq!(
            shifted_symbol_command(true, true, false, Key::_7),
            Some("rotate-0")
        );
        assert_eq!(
            shifted_symbol_command(true, true, false, Key::_0),
            Some("only-fit-oversized")
        );
        // Ctrl+Shift+OemMinus — Rotate Left (the shifted symbol is
        // 'underscore' on US layouts).
        assert_eq!(
            shifted_symbol_command(true, true, false, Key::minus),
            Some("rotate-left")
        );
        // Letters and lone-Ctrl combos never route here (the real
        // accelerators cover them).
        assert_eq!(shifted_symbol_command(true, true, false, Key::x), None);
        assert_eq!(shifted_symbol_command(true, false, false, Key::_4), None);
        // Ctrl+Shift+'=' (the Rotate Right key) is NOT in the map —
        // the raw keyval 'plus' matches the real accelerator.
        assert_eq!(shifted_symbol_command(true, true, false, Key::equal), None);
    }
}
