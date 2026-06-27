//! Drawing-dimension conversion.
//!
//! `core` provides `TryFrom<u16>` for `NonZeroU16` but no `TryFrom<i16>`, so a
//! computed signed size would not convert. This sealed trait accepts the integer
//! types a cart actually holds — a literal, a computed coordinate difference, or
//! an explicit value — and validates each to a strictly positive size.

use core::num::NonZeroU16;

mod sealed {
    pub trait Sealed {}
}

/// A drawing dimension (width, height, radius, tile count).
///
/// Implemented for `i16`, `u16`, `i32`, `u32` and `NonZeroU16`, so a draw call
/// accepts a literal, a value computed from positions, or an already-validated
/// size. The wider `i32`/`u32` impls keep unsuffixed literals (which default to
/// `i32`) and computed `i32`/`u32` differences usable without a suffix.
pub trait Dim: sealed::Sealed {
    /// The dimension as a strictly positive value, or `None` if it was zero,
    /// negative, or larger than `u16::MAX`.
    fn to_nonzero(self) -> Option<NonZeroU16>;
}

impl sealed::Sealed for i16 {}
impl Dim for i16 {
    fn to_nonzero(self) -> Option<NonZeroU16> {
        u16::try_from(self).ok().and_then(NonZeroU16::new)
    }
}

impl sealed::Sealed for u16 {}
impl Dim for u16 {
    fn to_nonzero(self) -> Option<NonZeroU16> {
        NonZeroU16::new(self)
    }
}

impl sealed::Sealed for i32 {}
impl Dim for i32 {
    fn to_nonzero(self) -> Option<NonZeroU16> {
        u16::try_from(self).ok().and_then(NonZeroU16::new)
    }
}

impl sealed::Sealed for u32 {}
impl Dim for u32 {
    fn to_nonzero(self) -> Option<NonZeroU16> {
        u16::try_from(self).ok().and_then(NonZeroU16::new)
    }
}

impl sealed::Sealed for NonZeroU16 {}
impl Dim for NonZeroU16 {
    fn to_nonzero(self) -> Option<NonZeroU16> {
        Some(self)
    }
}

/// A draw call was given a size that was not strictly positive (zero or
/// negative). The call drew nothing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ZeroSize;

#[cfg(test)]
mod tests {
    use super::*;
    use core::num::NonZeroU16;

    #[test]
    fn i16_conversions() {
        assert_eq!(5i16.to_nonzero(), NonZeroU16::new(5));
        assert_eq!(0i16.to_nonzero(), None);
        assert_eq!((-3i16).to_nonzero(), None);
    }

    #[test]
    fn u16_conversions() {
        assert_eq!(5u16.to_nonzero(), NonZeroU16::new(5));
        assert_eq!(0u16.to_nonzero(), None);
    }

    #[test]
    fn i32_conversions() {
        assert_eq!(5i32.to_nonzero(), NonZeroU16::new(5));
        assert_eq!(0i32.to_nonzero(), None);
        assert_eq!((-3i32).to_nonzero(), None);
        // Larger than u16::MAX does not fit.
        assert_eq!(70000i32.to_nonzero(), None);
    }

    #[test]
    fn nonzero_passes_through() {
        let n = NonZeroU16::new(7).unwrap();
        assert_eq!(n.to_nonzero(), Some(n));
    }
}
