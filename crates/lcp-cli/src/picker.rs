//! Interactive terminal picker (spec §11.9): raw-mode key navigation over a list of messages,
//! with the terminal always restored on the way out, including panics.

use std::io::Write;

use crossterm::cursor;
use crossterm::event::{self, Event, KeyCode, KeyEventKind, KeyModifiers};
use crossterm::style::Print;
use crossterm::terminal::{self, disable_raw_mode, enable_raw_mode};
use crossterm::{execute, queue};

pub struct PickerRow {
    pub sender_label: String,
    pub received_at_unix_ms: i64,
    pub text: String,
}

pub enum PickOutcome {
    Selected(String),
    Cancelled,
    Interrupted,
}

struct RawModeGuard;

impl RawModeGuard {
    fn enable() -> anyhow::Result<Self> {
        enable_raw_mode()?;
        execute!(std::io::stdout(), cursor::Hide)?;
        Ok(Self)
    }
}

impl Drop for RawModeGuard {
    fn drop(&mut self) {
        let _ = execute!(std::io::stdout(), cursor::Show);
        let _ = disable_raw_mode();
    }
}

pub fn run(title: &str, rows: &[PickerRow]) -> anyhow::Result<PickOutcome> {
    if rows.is_empty() {
        return Ok(PickOutcome::Cancelled);
    }

    let _guard = RawModeGuard::enable()?;
    let mut stdout = std::io::stdout();
    let mut selected = 0usize;

    loop {
        render(&mut stdout, title, rows, selected)?;
        let Event::Key(key) = event::read()? else {
            continue;
        };
        if key.kind != KeyEventKind::Press {
            continue;
        }
        match key.code {
            KeyCode::Up => selected = selected.saturating_sub(1),
            KeyCode::Down => selected = (selected + 1).min(rows.len() - 1),
            KeyCode::Enter => return Ok(PickOutcome::Selected(rows[selected].text.clone())),
            KeyCode::Esc => return Ok(PickOutcome::Cancelled),
            KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                return Ok(PickOutcome::Interrupted)
            }
            _ => {}
        }
    }
}

fn render(
    stdout: &mut std::io::Stdout,
    title: &str,
    rows: &[PickerRow],
    selected: usize,
) -> anyhow::Result<()> {
    queue!(
        stdout,
        terminal::Clear(terminal::ClearType::All),
        cursor::MoveTo(0, 0),
        Print(title),
        Print("\r\n\r\n"),
    )?;

    let width = terminal::size().map(|(w, _)| w as usize).unwrap_or(80);
    let time_width = 7usize;
    let sender_width = 9usize;
    let preview_width = width.saturating_sub(2 + time_width + sender_width).max(20);

    for (i, row) in rows.iter().enumerate() {
        let marker = if i == selected { "\u{276f} " } else { "  " };
        let time = format_local_hhmm(row.received_at_unix_ms);
        let preview = make_preview(&row.text, preview_width);
        queue!(
            stdout,
            Print(format!(
                "{marker}{}{}{preview}",
                fit(&time, time_width),
                fit(&row.sender_label, sender_width),
            )),
            Print("\r\n"),
        )?;
    }

    queue!(
        stdout,
        Print("\r\n\u{2191}/\u{2193} Select   Enter Copy   Esc Cancel\r\n")
    )?;
    stdout.flush()?;
    Ok(())
}

/// Pads/truncates to exactly `width` visible characters, so a fixed-column layout never glues
/// into the next column the way a plain `{:<width$}` would once content exceeds that width
/// (e.g. a long peer alias butting straight into the preview text).
fn fit(text: &str, width: usize) -> String {
    let char_count = text.chars().count();
    if char_count <= width {
        format!("{text:<width$}")
    } else {
        let truncated: String = text.chars().take(width.saturating_sub(1)).collect();
        format!("{truncated}\u{2026}")
    }
}

/// Collapses newline/tab/repeated whitespace to one space, then truncates to `max_width`
/// characters (minimum 20, spec §11.9), appending `...` only when actually truncated. Only
/// affects the preview -- the full message returned by [`run`] is never altered.
fn make_preview(text: &str, max_width: usize) -> String {
    let collapsed: String = text.split_whitespace().collect::<Vec<_>>().join(" ");
    let max_width = max_width.max(20);
    if collapsed.chars().count() <= max_width {
        collapsed
    } else {
        let truncated: String = collapsed
            .chars()
            .take(max_width.saturating_sub(3))
            .collect();
        format!("{truncated}...")
    }
}

fn format_local_hhmm(unix_ms: i64) -> String {
    chrono::DateTime::from_timestamp_millis(unix_ms)
        .map(|dt| dt.with_timezone(&chrono::Local).format("%H:%M").to_string())
        .unwrap_or_else(|| "--:--".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preview_collapses_internal_whitespace() {
        assert_eq!(
            make_preview("hello\n\tworld   there", 80),
            "hello world there"
        );
    }

    #[test]
    fn preview_truncates_and_appends_ellipsis() {
        let long = "a".repeat(50);
        let preview = make_preview(&long, 20);
        assert_eq!(preview.chars().count(), 20);
        assert!(preview.ends_with("..."));
    }

    #[test]
    fn preview_leaves_short_text_untouched() {
        assert_eq!(make_preview("short", 80), "short");
    }

    #[test]
    fn preview_width_has_a_20_char_floor() {
        let long = "a".repeat(50);
        let preview = make_preview(&long, 5);
        assert_eq!(preview.chars().count(), 20);
    }
}
