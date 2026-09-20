//! The pre-flight dialog for "Update Comic Vine Cache" (ADR-075).
//!
//! An update contacts Comic Vine, which is rate-limited (about 200
//! requests per resource per hour). A user far behind can trigger a run
//! of many hours that pauses to respect the limit. This dialog shows
//! the scale first — the changed-row count per endpoint since each
//! watermark, from a cheap pre-flight probe — and lets the user bound
//! the run with a per-endpoint page cap before it starts. The run is
//! resumable, so a capped or interrupted run continues next time.

use gtk4::prelude::*;
use gtk4::{Dialog, Label, Orientation, SpinButton};

use cr_scrape::cache::update::EndpointEstimate;

/// A page is 100 rows; the budget is about 200 requests per resource
/// per hour, so two pages is roughly one request pair. The estimate
/// uses the configured rate limit for the hour figure.
const PAGE_SIZE: i64 = 100;

/// The user's choice on Start: the per-endpoint page cap, where `None`
/// means run to the end of every window.
pub type UpdateChoice = Option<usize>;

/// Opens the modal pre-flight dialog. `estimates` come from
/// `cache::update::preflight`. `default_cap` is the configured
/// `CACHE_UPDATE_MAX_PAGES` (0 means "to completion"). `rate_limit` is
/// the per-resource hourly budget, for the time estimate. `on_done`
/// receives `Some(cap)` on Start (cap `None` = to completion) or `None`
/// on Cancel.
pub fn show(
    parent: &impl IsA<gtk4::Window>,
    estimates: Vec<EndpointEstimate>,
    default_cap: i32,
    rate_limit: i32,
    on_done: impl Fn(Option<UpdateChoice>) + 'static,
) {
    let dialog = Dialog::builder()
        .title("Update Comic Vine Cache")
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

    let intro = Label::new(Some(
        "This contacts Comic Vine, which limits requests per resource \
         per hour. The counts below changed since the cache last \
         synced each resource.",
    ));
    intro.set_wrap(true);
    intro.set_xalign(0.0);
    intro.set_max_width_chars(56);
    content.append(&intro);

    let total_changed: i64 = estimates.iter().map(|e| e.changed.max(0)).sum();
    for est in &estimates {
        let line = Label::new(Some(&format!(
            "{}: {} changed since {}",
            est.endpoint,
            est.changed,
            if est.since.is_empty() {
                "the start"
            } else {
                est.since.as_str()
            },
        )));
        line.set_xalign(0.0);
        content.append(&line);
    }

    // The rough time at the rate limit: the heaviest endpoint sets the
    // wall-clock, because each resource has its own hourly budget.
    let heaviest = estimates
        .iter()
        .map(|e| e.changed.max(0))
        .max()
        .unwrap_or(0);
    let hours = time_estimate_hours(heaviest, rate_limit);
    let est_line = Label::new(Some(&format!(
        "About {hours} at the rate limit ({rate_limit} requests per \
         resource per hour). The run is resumable — you can stop it and \
         continue later.",
    )));
    est_line.set_wrap(true);
    est_line.set_xalign(0.0);
    est_line.set_max_width_chars(56);
    content.append(&est_line);

    // The page cap. Zero means run to completion.
    let cap_row = gtk4::Box::new(Orientation::Horizontal, 8);
    cap_row.append(&Label::new(Some(
        "Stop after N pages per resource (0 = all):",
    )));
    let spin = SpinButton::with_range(0.0, 100_000.0, 1.0);
    spin.set_digits(0);
    spin.set_value(f64::from(default_cap.max(0)));
    spin.set_hexpand(true);
    cap_row.append(&spin);
    content.append(&cap_row);

    let cap_note = Label::new(Some(&format!(
        "A page is {PAGE_SIZE} rows. {total_changed} rows changed in total.",
    )));
    cap_note.set_xalign(0.0);
    content.append(&cap_note);

    dialog.add_button("Cancel", gtk4::ResponseType::Cancel);
    let start = dialog.add_button("Start", gtk4::ResponseType::Ok);
    dialog.set_default_widget(Some(&start));

    dialog.connect_response(move |dlg, response| {
        let result = match response {
            gtk4::ResponseType::Ok => {
                let pages = spin.value() as i64;
                // Zero means run to the end of every window.
                Some(if pages <= 0 {
                    None
                } else {
                    Some(pages as usize)
                })
            }
            _ => None,
        };
        dlg.close();
        on_done(result);
    });
    dialog.present();
}

/// A plain-language time figure for `changed` rows at `rate_limit`
/// requests per hour. One request returns one page of `PAGE_SIZE`
/// rows, so the request count is `ceil(changed / PAGE_SIZE)`.
pub fn time_estimate_hours(changed: i64, rate_limit: i32) -> String {
    let rate = i64::from(rate_limit).max(1);
    let rows = changed.max(0);
    let requests = (rows + PAGE_SIZE - 1) / PAGE_SIZE;
    if requests <= rate {
        return "under an hour".to_string();
    }
    let hours = (requests + rate - 1) / rate;
    if hours == 1 {
        "1 hour".to_string()
    } else {
        format!("{hours} hours")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_small_window_is_under_an_hour() {
        // 500 rows -> 5 requests, well under 200/hour.
        assert_eq!(time_estimate_hours(500, 200), "under an hour");
        assert_eq!(time_estimate_hours(0, 200), "under an hour");
    }

    #[test]
    fn a_large_window_reports_hours() {
        // 60,000 rows -> 600 requests; at 200/hour that is 3 hours.
        assert_eq!(time_estimate_hours(60_000, 200), "3 hours");
        // 20,000 rows -> 200 requests -> exactly one hour's budget.
        assert_eq!(time_estimate_hours(20_000, 200), "under an hour");
        // 20,001 rows -> 201 requests -> spills into a second hour.
        assert_eq!(time_estimate_hours(20_001, 200), "2 hours");
    }
}
