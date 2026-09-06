//! The Custom Zoom dialog — the `Dialogs/ZoomDialog.cs` port.
//!
//! The C# dialog: a "Percentage zoom:" label, a numeric field
//! (100..800, step 10), OK/Cancel. `Show` returns the new zoom on OK,
//! the original otherwise. The value setter clamps the incoming zoom
//! into the field range (`numPercentage.Value`).

use gtk4::prelude::*;
use gtk4::{Dialog, Label, Orientation, SpinButton};

/// The field range (`numPercentage.Minimum` / `Maximum`).
pub const MIN_ZOOM_PERCENT: f64 = 100.0;
pub const MAX_ZOOM_PERCENT: f64 = 800.0;

/// The OK zoom (the C# `ZoomDialog.Show` result: the new value, or
/// None when cancelled — the caller keeps the original).
pub type ZoomResult = f32;

/// Opens the modal Custom Zoom dialog over `zoom` (`Show(parent,
/// zoom)`); `on_done` receives `Some(new_zoom)` on OK, None on
/// Cancel.
pub fn show_zoom_dialog(
    parent: &impl IsA<gtk4::Window>,
    zoom: f32,
    on_done: impl Fn(Option<ZoomResult>) + 'static,
) {
    let dialog = Dialog::builder()
        .title("Custom Zoom")
        .transient_for(parent)
        .modal(true)
        .resizable(false)
        .build();
    let content = dialog.content_area();
    content.set_margin_top(12);
    content.set_margin_bottom(12);
    content.set_margin_start(12);
    content.set_margin_end(12);
    content.set_spacing(10);

    let row = gtk4::Box::new(Orientation::Horizontal, 8);
    row.append(&Label::new(Some("Percentage zoom:")));
    // The numeric field (100..800, the C# Increment 10, right
    // aligned).
    let spin = SpinButton::with_range(MIN_ZOOM_PERCENT, MAX_ZOOM_PERCENT, 10.0);
    spin.set_digits(0);
    spin.set_value(clamp_percent(zoom));
    spin.set_hexpand(true);
    row.append(&spin);
    content.append(&row);

    dialog.add_button("Cancel", gtk4::ResponseType::Cancel);
    let ok = dialog.add_button("OK", gtk4::ResponseType::Ok);
    dialog.set_default_widget(Some(&ok));

    dialog.connect_response(move |dlg, response| {
        let result = match response {
            gtk4::ResponseType::Ok => Some(spin.value() as f32 / 100.0),
            _ => None,
        };
        dlg.close();
        on_done(result);
    });
    dialog.present();
}

/// The incoming-zoom clamp of the C# `Zoom` setter (`(int)(value *
/// 100).Clamp(Minimum, Maximum)`; the integer cast truncates).
pub fn clamp_percent(zoom: f32) -> f64 {
    ((zoom * 100.0) as i32).clamp(MIN_ZOOM_PERCENT as i32, MAX_ZOOM_PERCENT as i32) as f64
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clamps_like_the_c_sharp_setter() {
        assert_eq!(clamp_percent(1.0), 100.0);
        assert_eq!(clamp_percent(2.0), 200.0);
        assert_eq!(clamp_percent(0.5), 100.0);
        assert_eq!(clamp_percent(9.0), 800.0);
        // The integer cast truncates (2.555 → 255).
        assert_eq!(clamp_percent(2.555), 255.0);
    }
}
