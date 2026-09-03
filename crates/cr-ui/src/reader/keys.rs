//! The reader command table — the port of `MainForm.InitializeKeyboard`
//! (`ComicRack/MainForm.cs`, the `ComicDisplay.KeyboardMap.Commands`
//! registrations) plus the `CommandKey`/`KeyboardShortcuts` machinery
//! (`cYo.Common.Windows`).
//!
//! Dispatch is `KeyboardShortcuts.HandleKey` parity: an exact match on
//! key + modifiers (`KeyboardCommand.Handles`), first registration
//! wins. The table order below IS the C# registration order — do not
//! reorder it. Gesture/touch-only commands (`ToggleNavigationOverlay`,
//! `ToggleZoom`) have no desktop binding and are not listed.

use gtk4::gdk;

/// A key or pointer input plus modifiers (`CommandKey`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CommandKey {
    pub key: Key,
    pub mods: Mods,
}

/// Modifier bits (`CommandKey.Ctrl/Shift/Alt`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Mods {
    pub ctrl: bool,
    pub shift: bool,
    pub alt: bool,
}

/// The key half of `CommandKey`: letters, digits, navigation keys and
/// the mouse/wheel pseudo-keys the reader table binds.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Key {
    A,
    B,
    C,
    D,
    E,
    F,
    G,
    H,
    I,
    J,
    K,
    L,
    M,
    N,
    O,
    P,
    Q,
    R,
    S,
    T,
    U,
    V,
    W,
    X,
    Y,
    Z,
    D0,
    D1,
    D2,
    D3,
    D4,
    D5,
    D6,
    D7,
    D8,
    D9,
    Up,
    Down,
    Left,
    Right,
    Tab,
    Home,
    End,
    PageUp,
    PageDown,
    Space,
    Escape,
    MouseLeft,
    MouseDoubleLeft,
    MouseWheelUp,
    MouseWheelDown,
    MouseTiltLeft,
    MouseTiltRight,
}

/// One reader command: the C# `KeyboardCommand` (id, group, caption,
/// default keys). `id` is the C# `Id` — the dispatch switch in
/// `page_view` matches on it.
#[derive(Debug, PartialEq, Eq)]
pub struct Command {
    pub id: &'static str,
    pub group: &'static str,
    pub text: &'static str,
    pub keys: &'static [CommandKey],
}

const fn ck(key: Key) -> CommandKey {
    CommandKey {
        key,
        mods: Mods {
            ctrl: false,
            shift: false,
            alt: false,
        },
    }
}

const fn ckm(key: Key, ctrl: bool, shift: bool, alt: bool) -> CommandKey {
    CommandKey {
        key,
        mods: Mods { ctrl, shift, alt },
    }
}

/// The `MainForm.InitializeKeyboard` reader commands in registration
/// order (`InitializeKeyboard`, MainForm.cs). Handlers live in
/// `page_view::dispatch_command`; commands for features that land in
/// T5/T6 or later phases dispatch as no-ops until then.
pub const COMMANDS: &[Command] = &[
    // Library
    Command {
        id: "NextComic",
        group: "Library",
        text: "Next Book",
        keys: &[ck(Key::N)],
    },
    Command {
        id: "PrevComic",
        group: "Library",
        text: "Previous Book",
        keys: &[ck(Key::P)],
    },
    Command {
        id: "RandomComic",
        group: "Library",
        text: "Random Book",
        keys: &[ck(Key::L)],
    },
    Command {
        id: "ShowBrowser",
        group: "Library",
        text: "Show Browser",
        keys: &[ck(Key::MouseLeft), ck(Key::Escape)],
    },
    // Browse
    Command {
        id: "MoveToFirstPage",
        group: "Browse",
        text: "First Page",
        keys: &[ckm(Key::Home, true, false, false)],
    },
    Command {
        id: "MoveToPreviousPage",
        group: "Browse",
        text: "Previous Page",
        keys: &[ck(Key::PageUp), ckm(Key::Left, false, false, true)],
    },
    Command {
        id: "MoveToNextPage",
        group: "Browse",
        text: "Next Page",
        keys: &[ck(Key::PageDown), ckm(Key::Right, false, false, true)],
    },
    Command {
        id: "MoveToLastPage",
        group: "Browse",
        text: "Last Page",
        keys: &[ckm(Key::End, true, false, false)],
    },
    Command {
        id: "MoveToPrevBookmark",
        group: "Browse",
        text: "Previous Bookmark",
        keys: &[ckm(Key::PageUp, true, false, false)],
    },
    Command {
        id: "MoveToNextBookmark",
        group: "Browse",
        text: "Next Bookmark",
        keys: &[ckm(Key::PageDown, true, false, false)],
    },
    Command {
        id: "PrevTab",
        group: "Browse",
        text: "Previous Tab",
        keys: &[ckm(Key::Tab, false, true, false)],
    },
    Command {
        id: "NextTab",
        group: "Browse",
        text: "Next Tab",
        keys: &[ck(Key::Tab)],
    },
    Command {
        id: "MoveToPrevPageSingle",
        group: "Browse",
        text: "Single Page Back",
        keys: &[ckm(Key::PageUp, false, true, false)],
    },
    Command {
        id: "MoveToNextPageSingle",
        group: "Browse",
        text: "Single Page Forward",
        keys: &[ckm(Key::PageDown, false, true, false)],
    },
    // Auto Scroll
    Command {
        id: "MovePrevPart",
        group: "Auto Scroll",
        text: "Previous Part",
        keys: &[ckm(Key::Space, false, true, false)],
    },
    Command {
        id: "MoveNextPart",
        group: "Auto Scroll",
        text: "Next Part",
        keys: &[ck(Key::Space)],
    },
    Command {
        id: "MoveFirstPart",
        group: "Auto Scroll",
        text: "Page Start",
        keys: &[ck(Key::Home)],
    },
    Command {
        id: "MoveLastPart",
        group: "Auto Scroll",
        text: "Page End",
        keys: &[ck(Key::End)],
    },
    Command {
        id: "MovePartDown10",
        group: "Auto Scroll",
        text: "Move Part 10% down",
        keys: &[ck(Key::V), ckm(Key::Down, true, false, false)],
    },
    Command {
        id: "MovePartUp10",
        group: "Auto Scroll",
        text: "Move Part 10% up",
        keys: &[ck(Key::B), ckm(Key::Up, true, false, false)],
    },
    // Scroll
    Command {
        id: "ToggleAutoScrolling",
        group: "Scroll",
        text: "Toggle Auto Scrolling",
        keys: &[ck(Key::S)],
    },
    Command {
        id: "DoublePageAutoScroll",
        group: "Scroll",
        text: "Double Page Auto Scroll",
        keys: &[ckm(Key::S, false, true, false)],
    },
    Command {
        id: "MoveUp",
        group: "Scroll",
        text: "Up",
        keys: &[ck(Key::Up), ck(Key::MouseWheelUp)],
    },
    Command {
        id: "MoveDown",
        group: "Scroll",
        text: "Down",
        keys: &[ck(Key::Down), ck(Key::MouseWheelDown)],
    },
    Command {
        id: "MoveLeft",
        group: "Scroll",
        text: "Left",
        keys: &[ck(Key::Left), ck(Key::MouseTiltLeft)],
    },
    Command {
        id: "MoveRight",
        group: "Scroll",
        text: "Right",
        keys: &[ck(Key::Right), ck(Key::MouseTiltRight)],
    },
    // Display Options
    Command {
        id: "ToggleUndockReader",
        group: "Display Options",
        text: "Toggle Undock Reader",
        keys: &[ck(Key::D)],
    },
    Command {
        id: "ToggleFullScreen",
        group: "Display Options",
        text: "Toggle Full Screen",
        keys: &[ck(Key::F), ck(Key::MouseDoubleLeft)],
    },
    Command {
        id: "ToggleTwoPages",
        group: "Display Options",
        text: "Toggle Two Pages",
        keys: &[ck(Key::T)],
    },
    Command {
        id: "ToggleRealisticPages",
        group: "Display Options",
        text: "Toggle Realistic Display",
        keys: &[ckm(Key::D, false, true, false)],
    },
    Command {
        id: "ToggleMagnify",
        group: "Display Options",
        text: "Toggle Magnifier",
        keys: &[ck(Key::M)],
    },
    Command {
        id: "ToggleMenu",
        group: "Display Options",
        text: "Toggle Menu",
        keys: &[ck(Key::K)],
    },
    // Page Display
    Command {
        id: "Original",
        group: "Page Display",
        text: "Original Size",
        keys: &[ck(Key::D1)],
    },
    Command {
        id: "FitAll",
        group: "Page Display",
        text: "Fit All",
        keys: &[ck(Key::D2)],
    },
    Command {
        id: "FitWidth",
        group: "Page Display",
        text: "Fit Width",
        keys: &[ck(Key::D3)],
    },
    Command {
        id: "FitWidthAdaptive",
        group: "Page Display",
        text: "Fit Width (adaptive)",
        keys: &[ck(Key::D4)],
    },
    Command {
        id: "FitHeight",
        group: "Page Display",
        text: "Fit Height",
        keys: &[ck(Key::D5)],
    },
    Command {
        id: "FitBest",
        group: "Page Display",
        text: "Best Fit",
        keys: &[ck(Key::D6)],
    },
    Command {
        id: "SinglePage",
        group: "Page Display",
        text: "Single Page",
        keys: &[ck(Key::D7)],
    },
    Command {
        id: "TwoPages",
        group: "Page Display",
        text: "Two Pages",
        keys: &[ck(Key::D8)],
    },
    Command {
        id: "TwoPagesAdaptive",
        group: "Page Display",
        text: "Two Pages (adaptive)",
        keys: &[ck(Key::D9)],
    },
    // "Continuous" is registered in the C# with no default key.
    Command {
        id: "RightToLeft",
        group: "Page Display",
        text: "Right to Left",
        keys: &[ck(Key::D0)],
    },
    Command {
        id: "OnlyFitIfOversized",
        group: "Page Display",
        text: "Only Fit if oversized",
        keys: &[ck(Key::O)],
    },
    // ZoomAndRotate
    Command {
        id: "RotateC",
        group: "ZoomAndRotate",
        text: "Rotate Right",
        keys: &[ck(Key::R)],
    },
    Command {
        id: "RotateCC",
        group: "ZoomAndRotate",
        text: "Rotate Left",
        keys: &[ckm(Key::R, false, true, false)],
    },
    Command {
        id: "AutoRotate",
        group: "ZoomAndRotate",
        text: "Autorotate Double Pages",
        keys: &[ck(Key::A)],
    },
    Command {
        id: "ZoomIn",
        group: "ZoomAndRotate",
        text: "Zoom In",
        keys: &[ckm(Key::MouseWheelUp, true, false, false)],
    },
    Command {
        id: "ZoomOut",
        group: "ZoomAndRotate",
        text: "Zoom Out",
        keys: &[ckm(Key::MouseWheelDown, true, false, false)],
    },
    Command {
        id: "StepZoomIn",
        group: "ZoomAndRotate",
        text: "Step Zoom In",
        keys: &[ck(Key::Z)],
    },
    Command {
        id: "StepZoomOut",
        group: "ZoomAndRotate",
        text: "Step Zoom Out",
        keys: &[ckm(Key::Z, false, true, false)],
    },
    // Edit
    Command {
        id: "PageRotateC",
        group: "Edit",
        text: "Rotate Page Right",
        keys: &[ck(Key::Y)],
    },
    Command {
        id: "PageRotateCC",
        group: "Edit",
        text: "Rotate Page Left",
        keys: &[ckm(Key::Y, false, true, false)],
    },
    // Other
    Command {
        id: "Exit",
        group: "Other",
        text: "Exit",
        keys: &[ck(Key::Q)],
    },
];

/// `KeyboardShortcuts.HandleKey` — exact match on key + modifiers,
/// first registration wins.
pub fn command_for(ckey: CommandKey) -> Option<&'static Command> {
    COMMANDS.iter().find(|c| c.keys.contains(&ckey))
}

/// `CommandKey` from a GDK keyval + modifier state (the
/// `ReaderFormKeyDown` path). Letters map case-insensitively — the
/// shift state rides in `mods`, like the WinForms `KeyCode`.
pub fn command_key_from_gdk(key: gdk::Key, state: gdk::ModifierType) -> Option<CommandKey> {
    let base = match key {
        gdk::Key::a | gdk::Key::A => Key::A,
        gdk::Key::b | gdk::Key::B => Key::B,
        gdk::Key::c | gdk::Key::C => Key::C,
        gdk::Key::d | gdk::Key::D => Key::D,
        gdk::Key::e | gdk::Key::E => Key::E,
        gdk::Key::f | gdk::Key::F => Key::F,
        gdk::Key::g | gdk::Key::G => Key::G,
        gdk::Key::h | gdk::Key::H => Key::H,
        gdk::Key::i | gdk::Key::I => Key::I,
        gdk::Key::j | gdk::Key::J => Key::J,
        gdk::Key::k | gdk::Key::K => Key::K,
        gdk::Key::l | gdk::Key::L => Key::L,
        gdk::Key::m | gdk::Key::M => Key::M,
        gdk::Key::n | gdk::Key::N => Key::N,
        gdk::Key::o | gdk::Key::O => Key::O,
        gdk::Key::p | gdk::Key::P => Key::P,
        gdk::Key::q | gdk::Key::Q => Key::Q,
        gdk::Key::r | gdk::Key::R => Key::R,
        gdk::Key::s | gdk::Key::S => Key::S,
        gdk::Key::t | gdk::Key::T => Key::T,
        gdk::Key::u | gdk::Key::U => Key::U,
        gdk::Key::v | gdk::Key::V => Key::V,
        gdk::Key::w | gdk::Key::W => Key::W,
        gdk::Key::x | gdk::Key::X => Key::X,
        gdk::Key::y | gdk::Key::Y => Key::Y,
        gdk::Key::z | gdk::Key::Z => Key::Z,
        gdk::Key::_0 => Key::D0,
        gdk::Key::_1 => Key::D1,
        gdk::Key::_2 => Key::D2,
        gdk::Key::_3 => Key::D3,
        gdk::Key::_4 => Key::D4,
        gdk::Key::_5 => Key::D5,
        gdk::Key::_6 => Key::D6,
        gdk::Key::_7 => Key::D7,
        gdk::Key::_8 => Key::D8,
        gdk::Key::_9 => Key::D9,
        gdk::Key::Up => Key::Up,
        gdk::Key::Down => Key::Down,
        gdk::Key::Left => Key::Left,
        gdk::Key::Right => Key::Right,
        gdk::Key::Tab => Key::Tab,
        gdk::Key::Home => Key::Home,
        gdk::Key::End => Key::End,
        gdk::Key::Page_Up => Key::PageUp,
        gdk::Key::Page_Down => Key::PageDown,
        gdk::Key::space => Key::Space,
        gdk::Key::Escape => Key::Escape,
        _ => return None,
    };
    Some(CommandKey {
        key: base,
        mods: Mods {
            ctrl: state.contains(gdk::ModifierType::CONTROL_MASK),
            shift: state.contains(gdk::ModifierType::SHIFT_MASK),
            alt: state.contains(gdk::ModifierType::ALT_MASK),
        },
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn plain(key: Key) -> CommandKey {
        ck(key)
    }

    #[test]
    fn exact_modifier_match_resolves_page_navigation() {
        // Plain PageUp, Shift+PageUp and Ctrl+PageUp are three
        // different commands (exact Handles match, registration
        // order breaks ties).
        assert_eq!(
            command_for(ck(Key::PageUp)).map(|c| c.id),
            Some("MoveToPreviousPage")
        );
        assert_eq!(
            command_for(ckm(Key::PageUp, false, true, false)).map(|c| c.id),
            Some("MoveToPrevPageSingle")
        );
        assert_eq!(
            command_for(ckm(Key::PageUp, true, false, false)).map(|c| c.id),
            Some("MoveToPrevBookmark")
        );
    }

    #[test]
    fn unbound_modifier_combinations_fall_through() {
        assert_eq!(command_for(ckm(Key::F, true, false, false)), None);
        assert_eq!(command_for(ckm(Key::K, true, false, false)), None);
        assert_eq!(command_for(ckm(Key::N, false, true, false)), None);
    }

    #[test]
    fn wheel_commands_match_with_ctrl() {
        assert_eq!(
            command_for(ck(Key::MouseWheelUp)).map(|c| c.id),
            Some("MoveUp")
        );
        assert_eq!(
            command_for(ckm(Key::MouseWheelUp, true, false, false)).map(|c| c.id),
            Some("ZoomIn")
        );
        assert_eq!(
            command_for(ckm(Key::MouseWheelDown, true, false, false)).map(|c| c.id),
            Some("ZoomOut")
        );
    }

    #[test]
    fn fit_and_layout_digit_keys() {
        let expected = [
            (Key::D1, "Original"),
            (Key::D2, "FitAll"),
            (Key::D3, "FitWidth"),
            (Key::D4, "FitWidthAdaptive"),
            (Key::D5, "FitHeight"),
            (Key::D6, "FitBest"),
            (Key::D7, "SinglePage"),
            (Key::D8, "TwoPages"),
            (Key::D9, "TwoPagesAdaptive"),
            (Key::D0, "RightToLeft"),
        ];
        for (key, id) in expected {
            assert_eq!(command_for(ck(key)).map(|c| c.id), Some(id));
        }
        assert_eq!(
            command_for(plain(Key::Space)).map(|c| c.id),
            Some("MoveNextPart")
        );
        assert_eq!(
            command_for(ckm(Key::Space, false, true, false)).map(|c| c.id),
            Some("MovePrevPart")
        );
        assert_eq!(command_for(ck(Key::Q)).map(|c| c.id), Some("Exit"));
    }

    #[test]
    fn command_ids_are_unique() {
        let mut ids: Vec<&str> = COMMANDS.iter().map(|c| c.id).collect();
        let n = ids.len();
        ids.sort_unstable();
        ids.dedup();
        assert_eq!(ids.len(), n);
    }

    #[test]
    fn letters_map_case_insensitively_with_shift_modifier() {
        // The C# KeyDown reports the unshifted key code; the shift
        // state arrives separately. Both cases map to the same Key
        // with the shift bit set.
        let shifted = command_key_from_gdk(gdk::Key::R, gdk::ModifierType::SHIFT_MASK);
        assert_eq!(
            shifted.and_then(command_for).map(|c| c.id),
            Some("RotateCC")
        );
        let plain = command_key_from_gdk(gdk::Key::r, gdk::ModifierType::empty());
        assert_eq!(plain.and_then(command_for).map(|c| c.id), Some("RotateC"));
    }

    #[test]
    fn every_registered_key_resolves_to_its_own_command() {
        // No accidental shadowing: each default key must dispatch to
        // the command that declares it (first-match order).
        for command in COMMANDS {
            for ckey in command.keys {
                assert_eq!(
                    command_for(*ckey).map(|c| c.id),
                    Some(command.id),
                    "key {ckey:?} of {} is shadowed by an earlier command",
                    command.id
                );
            }
        }
    }
}
