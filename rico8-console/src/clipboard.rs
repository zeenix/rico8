//! System-clipboard text access, isolated behind one function so the rest of
//! the console (and its tests) never touches `arboard` directly.

use anyhow::{Context, Result};

/// The clipboard's current text, or an error if the clipboard is unavailable
/// (e.g. no display server) or holds no text.
pub fn read_text() -> Result<String> {
    arboard::Clipboard::new()
        .context("open the system clipboard")?
        .get_text()
        .context("read text from the clipboard")
}
