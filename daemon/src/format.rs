//! Terminal formatting helpers — ANSI colors and simple table rendering.
//! Auto-detects TTY to avoid polluting piped/redirected output.

use std::io::IsTerminal;

/// Whether stdout is a TTY (cached on first call).
fn is_tty() -> bool {
    std::io::stdout().is_terminal()
}

// ---------------------------------------------------------------------------
// ANSI color helpers — return input unchanged when not a TTY
// ---------------------------------------------------------------------------

pub fn bold(s: &str) -> String {
    if is_tty() {
        format!("\x1b[1m{}\x1b[0m", s)
    } else {
        s.to_string()
    }
}

pub fn green(s: &str) -> String {
    if is_tty() {
        format!("\x1b[32m{}\x1b[0m", s)
    } else {
        s.to_string()
    }
}

pub fn yellow(s: &str) -> String {
    if is_tty() {
        format!("\x1b[33m{}\x1b[0m", s)
    } else {
        s.to_string()
    }
}

pub fn red(s: &str) -> String {
    if is_tty() {
        format!("\x1b[31m{}\x1b[0m", s)
    } else {
        s.to_string()
    }
}

pub fn dim(s: &str) -> String {
    if is_tty() {
        format!("\x1b[2m{}\x1b[0m", s)
    } else {
        s.to_string()
    }
}

pub fn cyan(s: &str) -> String {
    if is_tty() {
        format!("\x1b[36m{}\x1b[0m", s)
    } else {
        s.to_string()
    }
}

// ---------------------------------------------------------------------------
// Status icons
// ---------------------------------------------------------------------------

pub fn status_icon(done: bool) -> &'static str {
    if is_tty() {
        if done { "\x1b[32m✓\x1b[0m" } else { "○" }
    } else {
        if done { "[x]" } else { "[ ]" }
    }
}

pub fn task_status_icon(status: &str) -> &'static str {
    if is_tty() {
        match status {
            "done" => "\x1b[32m✓\x1b[0m",
            "in_progress" | "in progress" => "\x1b[33m▶\x1b[0m",
            "blocked" => "\x1b[31m✗\x1b[0m",
            _ => "○", // todo
        }
    } else {
        match status {
            "done" => "[x]",
            "in_progress" | "in progress" => "[>]",
            "blocked" => "[!]",
            _ => "[ ]",
        }
    }
}

// ---------------------------------------------------------------------------
// Simple table rendering — padding-based, no external deps
// ---------------------------------------------------------------------------

/// Compute column widths from headers and rows.
pub fn column_widths(headers: &[&str], rows: &[Vec<String>]) -> Vec<usize> {
    let mut widths: Vec<usize> = headers.iter().map(|h| h.len()).collect();
    for row in rows {
        for (i, cell) in row.iter().enumerate() {
            if i < widths.len() {
                // Strip ANSI codes for width calculation
                let visible_len = strip_ansi_len(cell);
                widths[i] = widths[i].max(visible_len);
            }
        }
    }
    widths
}

/// Print a formatted table to stdout.
pub fn print_table(headers: &[&str], rows: &[Vec<String>]) {
    let widths = column_widths(headers, rows);

    // Header
    let header_line: Vec<String> = headers
        .iter()
        .zip(&widths)
        .map(|(h, w)| format!("{:<width$}", h, width = w))
        .collect();
    println!("  {}", bold(&header_line.join("  ")));

    // Separator
    let sep: Vec<String> = widths.iter().map(|w| "─".repeat(*w)).collect();
    println!("  {}", dim(&sep.join("──")));

    // Rows
    for row in rows {
        let cells: Vec<String> = row
            .iter()
            .zip(&widths)
            .map(|(cell, w)| {
                let visible = strip_ansi_len(cell);
                let padding = if *w > visible { w - visible } else { 0 };
                format!("{}{}", cell, " ".repeat(padding))
            })
            .collect();
        println!("  {}", cells.join("  "));
    }
}

/// Print a key-value pair (for status display).
pub fn print_kv(key: &str, value: &str) {
    println!("  {}: {}", dim(key), value);
}

/// ASCII progress bar for project stages.
pub fn stage_bar(current: Option<&str>) -> String {
    let stages = [
        "Investigation",
        "Implementation",
        "Review",
        "Integration",
        "Deployment",
    ];
    let current_idx = current.and_then(|c| {
        stages
            .iter()
            .position(|s| s.to_lowercase() == c.to_lowercase())
    });

    let parts: Vec<String> = stages
        .iter()
        .enumerate()
        .map(|(i, name)| match current_idx {
            Some(idx) if i < idx => green(name),
            Some(idx) if i == idx => bold(&yellow(name)),
            _ => dim(name),
        })
        .collect();

    parts.join(&dim(" > "))
}

/// Truncate a UUID to first 8 chars for display.
pub fn short_id(id: &str) -> String {
    id.chars().take(8).collect()
}

/// Strip ANSI escape codes and return the visible character count.
fn strip_ansi_len(s: &str) -> usize {
    let mut len = 0;
    let mut in_escape = false;
    for c in s.chars() {
        if in_escape {
            if c == 'm' {
                in_escape = false;
            }
        } else if c == '\x1b' {
            in_escape = true;
        } else {
            len += 1;
        }
    }
    len
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_strip_ansi_len() {
        assert_eq!(strip_ansi_len("hello"), 5);
        assert_eq!(strip_ansi_len("\x1b[32mhello\x1b[0m"), 5);
        assert_eq!(strip_ansi_len("\x1b[1m\x1b[33mAB\x1b[0m"), 2);
    }

    #[test]
    fn test_short_id() {
        assert_eq!(short_id("12345678-abcd-efgh-ijkl"), "12345678");
        assert_eq!(short_id("abc"), "abc");
    }

    #[test]
    fn test_column_widths() {
        let headers = vec!["Name", "Status"];
        let rows = vec![
            vec!["alice".to_string(), "active".to_string()],
            vec!["bob-longer-name".to_string(), "ok".to_string()],
        ];
        let widths = column_widths(&headers, &rows);
        assert_eq!(widths, vec![15, 6]);
    }
}
