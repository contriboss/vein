//! Small shared formatting/sanitization helpers used across the crate.

use std::time::Duration;

/// Tokio sleep in milliseconds: the sleeper for chrono-machines' async retry
/// driver.
pub async fn sleep_ms(ms: u64) {
    tokio::time::sleep(Duration::from_millis(ms)).await;
}

/// `a`, `a and b`, `a, b and c`, with `joint` for `and`.
#[must_use]
pub fn listed(parts: &[String], joint: &str) -> String {
    match parts {
        [] => String::new(),
        [one] => one.clone(),
        [rest @ .., last] => format!("{} {joint} {last}", rest.join(", ")),
    }
}

/// Formats a byte count as a human-readable size (e.g. `1.50 MB`).
#[must_use]
pub fn format_bytes(bytes: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KB", "MB", "GB", "TB"];
    if bytes == 0 {
        return "0 B".to_string();
    }
    let mut value = bytes as f64;
    let mut unit = 0;
    while value >= 1024.0 && unit < UNITS.len() - 1 {
        value /= 1024.0;
        unit += 1;
    }
    if unit == 0 {
        format!("{bytes} {}", UNITS[unit])
    } else {
        format!("{value:.2} {}", UNITS[unit])
    }
}

/// Replaces every character that is not ASCII-alphanumeric or present in
/// `extra` with `_`, falling back to `"artifact"` when the result is empty.
fn sanitize_chars(input: &str, extra: &[char]) -> String {
    let sanitized: String = input
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || extra.contains(&c) {
                c
            } else {
                '_'
            }
        })
        .collect();
    if sanitized.is_empty() {
        "artifact".to_string()
    } else {
        sanitized
    }
}

/// Sanitizes a string for safe use as a filename segment.
#[must_use]
pub fn sanitize_filename(input: &str) -> String {
    sanitize_chars(input, &['-', '_', '.'])
}

/// Sanitizes an npm path segment (allows `@` and `+` in addition to the
/// characters permitted by [`sanitize_filename`]).
#[must_use]
pub fn sanitize_npm_segment(input: &str) -> String {
    sanitize_chars(input, &['-', '_', '.', '@', '+'])
}
