//! The Missing Issues scope selector and Refresh control (Phase 19).
//!
//! Shown only while the Missing Issues navigator node is selected. The
//! gap report never auto-recomputes (docs/phases/phase-19.md, locked
//! decision 3) — this bar's Refresh button is its only trigger, and
//! the scope combo is the only way to narrow the pass to one smart
//! list's series instead of the whole library.

use gtk4::prelude::*;
use gtk4::{Button, ComboBoxText, Label};

use cr_core::xml::scalar::CrGuid;

/// The combo id for the "Whole Library" entry (never a valid `CrGuid`
/// text, so it cannot collide with a smart list id).
const WHOLE_LIBRARY: &str = "__library__";

#[derive(Clone)]
pub struct MissingIssuesBar {
    bar: gtk4::Box,
    scope_combo: ComboBoxText,
    refresh_button: Button,
    status_label: Label,
}

impl MissingIssuesBar {
    pub fn create() -> MissingIssuesBar {
        let bar = gtk4::Box::new(gtk4::Orientation::Horizontal, 6);
        bar.add_css_class("toolbar");
        bar.set_visible(false);

        bar.append(&Label::new(Some("Scope:")));
        let scope_combo = ComboBoxText::new();
        bar.append(&scope_combo);

        let refresh_button = Button::with_label("Refresh");
        bar.append(&refresh_button);

        let status_label = Label::new(Some("Not yet run"));
        status_label.set_hexpand(true);
        status_label.set_halign(gtk4::Align::Start);
        status_label.set_margin_start(8);
        bar.append(&status_label);

        MissingIssuesBar {
            bar,
            scope_combo,
            refresh_button,
            status_label,
        }
    }

    pub fn widget(&self) -> &gtk4::Box {
        &self.bar
    }

    /// Refills the scope combo from the current smart lists
    /// (`library::smart_list_scope_options`), keeping "Whole Library"
    /// first and the previous choice selected when it still exists.
    pub fn refill_scope(&self, options: &[(CrGuid, String)]) {
        let previous = self.chosen_scope();
        self.scope_combo.remove_all();
        self.scope_combo
            .append(Some(WHOLE_LIBRARY), "Whole Library");
        for (id, name) in options {
            self.scope_combo.append(Some(&id.to_string()), name);
        }
        let pos = previous
            .and_then(|id| options.iter().position(|(oid, _)| *oid == id))
            .map(|p| p as u32 + 1);
        self.scope_combo.set_active(Some(pos.unwrap_or(0)));
    }

    /// The chosen scope: `None` for the whole library, `Some(id)` for
    /// one smart list.
    pub fn chosen_scope(&self) -> Option<CrGuid> {
        let active = self.scope_combo.active_id()?;
        if active.as_str() == WHOLE_LIBRARY {
            None
        } else {
            CrGuid::parse(&active).ok()
        }
    }

    /// Probe/test: picks the combo entry for `scope` (`None` = Whole
    /// Library). A no-op if `scope` names an id `refill_scope` has not
    /// added yet.
    pub fn set_scope(&self, scope: Option<CrGuid>) {
        self.scope_combo.set_active_id(Some(
            scope
                .map(|id| id.to_string())
                .as_deref()
                .unwrap_or(WHOLE_LIBRARY),
        ));
    }

    pub fn connect_refresh<F: Fn() + 'static>(&self, f: F) {
        self.refresh_button.connect_clicked(move |_| f());
    }

    /// Probe: fires the Refresh button exactly as a click would.
    pub fn click_refresh(&self) {
        self.refresh_button.emit_clicked();
    }

    pub fn set_status(&self, text: &str) {
        self.status_label.set_label(text);
    }

    /// Probe: the status label's current text.
    pub fn status_text(&self) -> String {
        self.status_label.text().to_string()
    }
}
