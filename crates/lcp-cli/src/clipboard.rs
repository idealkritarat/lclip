//! System clipboard access (spec §12). Runs only in this foreground client process --
//! `lanclipd` never touches the clipboard.

use arboard::Clipboard;

pub fn read_text() -> anyhow::Result<String> {
    let mut clipboard = Clipboard::new()?;
    let text = clipboard.get_text()?;
    Ok(text)
}

pub fn write_text(text: &str) -> anyhow::Result<()> {
    let mut clipboard = Clipboard::new()?;
    clipboard.set_text(text.to_string())?;
    Ok(())
}
