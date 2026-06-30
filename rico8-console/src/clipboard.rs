//! System-clipboard text access, isolated behind two functions so the rest of
//! the console (and its tests) never touches `arboard` directly.
//!
//! On X11/Wayland the clipboard's contents are served by whichever process owns
//! the selection; when that owner is dropped, the contents are cleared. A
//! clipboard created and dropped around a single write therefore loses the text
//! before any app (including this one) can read it. So one `arboard::Clipboard`
//! is kept alive for the whole process — in a thread-local, as all clipboard
//! access happens on the console's main thread — and reused for every read and
//! write.

use std::cell::RefCell;

use anyhow::{Context, Result};

thread_local! {
    /// The process's live clipboard handle, opened on first use and kept alive
    /// so the selection it owns stays available to paste.
    static CLIPBOARD: RefCell<Option<arboard::Clipboard>> = const { RefCell::new(None) };
}

/// Run `f` with the long-lived clipboard, opening it on first use.
fn with_clipboard<T>(f: impl FnOnce(&mut arboard::Clipboard) -> Result<T>) -> Result<T> {
    CLIPBOARD.with_borrow_mut(|slot| {
        if slot.is_none() {
            *slot = Some(arboard::Clipboard::new().context("open the system clipboard")?);
        }
        f(slot.as_mut().expect("clipboard opened above"))
    })
}

/// The clipboard's current text, or an error if the clipboard is unavailable
/// (e.g. no display server) or holds no text.
pub fn read_text() -> Result<String> {
    with_clipboard(|c| c.get_text().context("read text from the clipboard"))
}

/// Replace the system clipboard's text. Errors if the clipboard is unavailable
/// (e.g. no display server). The clipboard handle is kept alive afterwards so
/// the selection's owner stays around to serve paste requests.
pub fn write_text(text: &str) -> Result<()> {
    with_clipboard(|c| {
        c.set_text(text.to_owned())
            .context("write text to the clipboard")
    })
}
