//! The name prompt — the `SelectItemDialog.GetName<T>` shape (a
//! caption, a prefilled value, OK/Cancel). The C# Set Bookmark flow
//! opens it for the bookmark name (`MainForm.SetBookmark`).

use gtk4::prelude::*;
use gtk4::{Dialog, Entry};

/// Opens the modal prompt. `on_ok` runs once with the trimmed text
/// (an empty entry is a legal value — the Remove Bookmark semantic).
pub fn show_name_prompt(
    parent: &impl IsA<gtk4::Window>,
    title: &str,
    value: &str,
    on_ok: impl Fn(String) + 'static,
) {
    let dialog = Dialog::builder()
        .title(title)
        .transient_for(parent)
        .modal(true)
        .default_width(420)
        .build();
    let entry = Entry::builder().text(value).hexpand(true).build();
    let content = dialog.content_area();
    content.set_margin_top(8);
    content.set_margin_bottom(8);
    content.set_margin_start(8);
    content.set_margin_end(8);
    content.append(&entry);
    dialog.add_button("OK", gtk4::ResponseType::Ok);
    dialog.add_button("Cancel", gtk4::ResponseType::Cancel);
    // Enter commits (the C# dialog's accept button).
    entry.set_activates_default(true);
    dialog.set_default_response(gtk4::ResponseType::Ok);
    let done = std::rc::Rc::new(std::cell::Cell::new(false));
    {
        let done = std::rc::Rc::clone(&done);
        dialog.connect_response(move |dlg, response| {
            if done.replace(true) {
                return;
            }
            let text = entry.text().trim().to_string();
            let ok = response == gtk4::ResponseType::Ok;
            dlg.close();
            if ok {
                on_ok(text);
            }
        });
    }
    dialog.present();
}
