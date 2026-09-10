//! Small shared widget helpers (the menu-row shapes the plain
//! popovers rebuild by hand).

use gtk4::prelude::*;

/// A frameless menu-item button with a LEFT-aligned label (the C#
/// `ContextMenuStrip` item shape). GtkButton centers its label by
/// default — every context menu row needs the explicit alignment.
pub fn menu_item_button(label: &str) -> gtk4::Button {
    let button = gtk4::Button::with_label(label);
    button.set_has_frame(false);
    button.set_halign(gtk4::Align::Fill);
    if let Some(label_widget) = button.child().and_downcast::<gtk4::Label>() {
        label_widget.set_halign(gtk4::Align::Start);
        label_widget.set_xalign(0.0);
    }
    button
}
