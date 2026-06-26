//! Allocation-free text formatting for the `printf!`/`logf!` macros.
//!
//! Carts have no allocator on `no_std` and the SDK takes no dependencies, so
//! formatted text is collected into a fixed stack buffer that implements
//! [`core::fmt::Write`]. Writes past the end are dropped.

/// Default buffer capacity: one full screen line.
///
/// The screen is [`SCREEN_WIDTH`](crate::SCREEN_WIDTH) pixels wide and the built-in
/// font advances four pixels per glyph, so 32 characters fill a line.
pub const LINE_CAP: usize = (crate::SCREEN_WIDTH / 4) as usize;

/// A fixed-capacity, allocation-free sink for formatted text.
///
/// `N` is the byte capacity. Text written past it is silently dropped, and the
/// buffer only ever holds whole characters, so [`as_str`](FmtBuf::as_str) is
/// always valid UTF-8.
pub struct FmtBuf<const N: usize> {
    bytes: [u8; N],
    len: usize,
}

impl<const N: usize> FmtBuf<N> {
    /// An empty buffer.
    pub const fn new() -> Self {
        FmtBuf {
            bytes: [0; N],
            len: 0,
        }
    }

    /// The text written so far.
    pub fn as_str(&self) -> &str {
        // `write_str` only ever appends whole characters, so this never fails.
        // The fallback keeps the method total instead of panicking.
        core::str::from_utf8(&self.bytes[..self.len]).unwrap_or("")
    }
}

impl<const N: usize> Default for FmtBuf<N> {
    fn default() -> Self {
        Self::new()
    }
}

impl<const N: usize> core::fmt::Write for FmtBuf<N> {
    fn write_str(&mut self, s: &str) -> core::fmt::Result {
        let room = N - self.len;
        // Copy as much as fits, then back off to a character boundary so a
        // multi-byte character is never split across the cap.
        let mut take = room.min(s.len());
        while take > 0 && !s.is_char_boundary(take) {
            take -= 1;
        }
        self.bytes[self.len..self.len + take].copy_from_slice(&s.as_bytes()[..take]);
        self.len += take;
        Ok(())
    }
}

/// Format `args` into a fresh [`FmtBuf`] of capacity `N`.
///
/// Used by the `printf!`/`logf!` macros; not part of the cart-facing API.
pub fn format_args_to_buf<const N: usize>(args: core::fmt::Arguments<'_>) -> FmtBuf<N> {
    use core::fmt::Write as _;
    let mut buf = FmtBuf::new();
    let _ = buf.write_fmt(args);
    buf
}

#[cfg(test)]
mod tests {
    use super::*;
    use core::fmt::Write as _;

    #[test]
    fn formats_and_reads_back() {
        let buf = format_args_to_buf::<32>(format_args!("coins {}", 7));
        assert_eq!(buf.as_str(), "coins 7");
    }

    #[test]
    fn exact_fit_keeps_everything() {
        // Exactly 32 ASCII chars fill the default line capacity.
        let s = "abcdefghijklmnopqrstuvwxyz012345";
        assert_eq!(s.len(), LINE_CAP);
        let mut buf = FmtBuf::<32>::new();
        buf.write_str(s).unwrap();
        assert_eq!(buf.as_str(), s);
    }

    #[test]
    fn overflow_truncates() {
        let mut buf = FmtBuf::<4>::new();
        buf.write_str("abcdef").unwrap();
        assert_eq!(buf.as_str(), "abcd");
    }

    #[test]
    fn multibyte_char_never_split() {
        // "é" is two bytes; with room for one it must be dropped whole.
        let mut buf = FmtBuf::<2>::new();
        buf.write_str("aé").unwrap();
        assert_eq!(buf.as_str(), "a");
    }

    #[test]
    fn multibyte_char_exact_fit() {
        let mut buf = FmtBuf::<3>::new();
        buf.write_str("aé").unwrap();
        assert_eq!(buf.as_str(), "aé");
    }

    #[test]
    fn line_cap_is_one_screen_line() {
        assert_eq!(LINE_CAP, (crate::SCREEN_WIDTH / 4) as usize);
        assert_eq!(LINE_CAP, 32);
    }
}
