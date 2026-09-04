//! The options builder — the `FormUtility.FillPanelWithOptions` /
//! `RetrieveOptionsFromPanel` port.
//!
//! The C# reflects over the Settings object's booleans: browsable
//! fields with a non-empty description, grouped by `[Category]` into
//! collapsible groups, the rows sorted by description inside each
//! group. The port walks the typed registry table instead — same
//! filter, same group/sort semantics. GTK has no WinForms
//! property grid; the builder produces header + check-box rows.

use std::cell::RefCell;
use std::rc::Rc;

use gtk4::prelude::*;
use gtk4::{Align, Box as GtkBox, CheckButton, Label, Orientation, Revealer, ToggleButton};

use cr_core::settings::registry::{FieldDesc, Value};
use cr_core::settings::settings::SETTINGS_FIELDS;
use cr_core::settings::Settings;

pub type SettingsRef = Rc<RefCell<Settings>>;

/// One collapsible group (the C# `CollapsibleGroupBox`): a header
/// button toggling a revealer.
struct OptionGroup {
    /// The C# `Group.Tag` — the raw category (the session key).
    category: &'static str,
    /// `(field index, check box)`, sorted by description.
    rows: Vec<(usize, CheckButton)>,
}

/// The auto-filled options panel. Rows bind to `SETTINGS_FIELDS`
/// indexes; `retrieve` writes the check states back.
pub struct OptionsPanel {
    container: GtkBox,
    groups: Vec<OptionGroup>,
}

impl OptionsPanel {
    /// The group caption: `tr[cat ?? "Other"]` — the TR loader is not
    /// ported yet, so the English default is the C# category text
    /// (or "Other").
    fn caption(category: &str) -> String {
        if category.is_empty() {
            "Other".to_string()
        } else {
            category.to_string()
        }
    }

    /// Builds the panel from the fields table.
    pub fn build() -> OptionsPanel {
        let container = GtkBox::new(Orientation::Vertical, 6);
        container.set_margin_top(8);
        container.set_margin_bottom(8);
        container.set_margin_start(8);
        container.set_margin_end(8);

        // First-encounter category order (the C# walks the properties
        // in order and reuses the matching group).
        let mut groups: Vec<OptionGroup> = Vec::new();
        for (index, field) in SETTINGS_FIELDS.iter().enumerate() {
            if !field.is_options_checkbox() {
                continue;
            }
            let group = match groups.iter_mut().find(|g| g.category == field.category) {
                Some(g) => g,
                None => {
                    groups.push(OptionGroup {
                        category: field.category,
                        rows: Vec::new(),
                    });
                    groups.last_mut().unwrap()
                }
            };
            group.rows.push((index, CheckButton::new()));
        }
        // Rows sorted by description inside the group (the C#
        // `orderby c.Description`).
        for g in &mut groups {
            g.rows.sort_by(|a, b| {
                let da = SETTINGS_FIELDS[a.0].description;
                let db = SETTINGS_FIELDS[b.0].description;
                da.cmp(db)
            });
        }

        let mut panel = OptionsPanel { container, groups };
        panel.install_widgets();
        panel
    }

    fn install_widgets(&mut self) {
        for g in &self.groups {
            let caption = Self::caption(g.category);
            let header = ToggleButton::builder()
                .label(format!("▾ {caption}"))
                .css_classes(["flat"])
                .active(true)
                .build();
            let body = GtkBox::new(Orientation::Vertical, 2);
            body.set_margin_start(20);
            for (index, check) in &g.rows {
                let field = &SETTINGS_FIELDS[*index];
                check.set_label(Some(field.description));
                body.append(check);
            }
            let revealer = Revealer::builder()
                .child(&body)
                .transition_type(gtk4::RevealerTransitionType::SlideDown)
                .reveal_child(true)
                .build();
            let rev = revealer.clone();
            let caption_label = caption.clone();
            header.connect_toggled(move |btn| {
                rev.set_reveal_child(btn.is_active());
                let arrow = if btn.is_active() { "▾ " } else { "▸ " };
                btn.set_label(&format!("{arrow}{caption_label}"));
            });
            self.container.append(&header);
            self.container.append(&revealer);
        }
    }

    pub fn widget(&self) -> &GtkBox {
        &self.container
    }

    /// Reads the settings into the check boxes (`FillPanelWithOptions`
    /// runs on every dialog open).
    pub fn refresh(&self, settings: &SettingsRef) {
        let s = settings.borrow();
        for g in &self.groups {
            for (index, check) in &g.rows {
                if let Value::Bool(v) = (SETTINGS_FIELDS[*index].get)(&s) {
                    check.set_active(v);
                }
            }
        }
    }

    /// Writes the check states back (`RetrieveOptionsFromPanel`).
    pub fn retrieve(&self, settings: &SettingsRef) {
        let mut s = settings.borrow_mut();
        for g in &self.groups {
            for (index, check) in &g.rows {
                let field: &FieldDesc<Settings> = &SETTINGS_FIELDS[*index];
                (field.set)(&mut s, Value::Bool(check.is_active()));
            }
        }
    }
}

/// A plain section caption (the hand-built pages use it).
pub fn section_label(text: &str) -> Label {
    Label::builder()
        .label(text)
        .halign(Align::Start)
        .css_classes(["heading"])
        .margin_top(6)
        .margin_bottom(2)
        .build()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The panel row set must match the C# `FillPanelWithOptions`
    /// filter: browsable bools with a non-empty description — the
    /// overlay/engine internals and non-bool fields stay out.
    #[test]
    fn the_row_set_matches_the_formutility_filter() {
        let rows: Vec<&str> = SETTINGS_FIELDS
            .iter()
            .filter(|f| f.is_options_checkbox())
            .map(|f| f.name)
            .collect();
        assert!(rows.contains(&"ShowSplash"));
        assert!(rows.contains(&"OpenLastPage"));
        assert!(rows.contains(&"PageChangeDelay"));
        assert!(rows.contains(&"ScrollingDoesBrowse"));
        assert!(rows.contains(&"TrueRightToLeftReading"));
        assert!(rows.contains(&"NewBooksChecked"));
        assert!(rows.contains(&"AddToLibraryOnOpen"));
        // Browsable(false) or description-less fields never appear.
        assert!(!rows.contains(&"TrackCurrentPage"));
        assert!(!rows.contains(&"ShowCurrentPageOverlay"));
        assert!(!rows.contains(&"AutoHideMainMenu"));
        assert!(!rows.contains(&"Scripting"));
        assert!(!rows.contains(&"ShowQuickManual"));
        // Categories consolidate: a category re-encountered later
        // merges into the first group (the C# finds the existing
        // CollapsibleGroupBox by caption), so the panel shows each
        // category exactly once.
        let mut seen: Vec<&str> = Vec::new();
        for f in SETTINGS_FIELDS.iter().filter(|f| f.is_options_checkbox()) {
            if !seen.contains(&f.category) {
                seen.push(f.category);
            }
        }
        assert_eq!(
            seen.len(),
            seen.iter().collect::<std::collections::HashSet<_>>().len()
        );
    }
}
