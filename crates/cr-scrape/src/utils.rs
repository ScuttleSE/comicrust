//! Small helpers ported from the plugin's `utils.py` — the subset the
//! current modules need. More ports land here as later tasks consume
//! them (natural keys, number-word conversion, …).

/// Port of `utils.is_number`: true when the string converts to a
/// number (Python `float()` semantics, whitespace tolerated).
pub fn is_number(s: &str) -> bool {
    s.trim().parse::<f64>().is_ok()
}

/// Python `str(float)` formatting: integral floats keep a `.0`
/// (`"21.0000000"` parses to `21.0` and formats back as `"21.0"`),
/// everything else uses the shortest round-trip form (`"7.1"`).
pub fn py_float_string(f: f64) -> String {
    if f.is_finite() && f.fract() == 0.0 {
        format!("{:.1}", f)
    } else {
        format!("{}", f)
    }
}
