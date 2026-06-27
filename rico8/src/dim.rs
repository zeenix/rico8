//! Drawing-dimension conversion.
//!
//! `core` provides `TryFrom<u32>` for `NonZeroU32` but no `TryFrom<i32>`, so a
//! computed `i32` size would not convert. This sealed trait accepts the integer
//! types a cart actually holds — a literal, a computed coordinate difference, or
//! an explicit value — and validates each to a strictly positive size.

use core::num::NonZeroU32;

mod sealed {
    pub trait Sealed {}
}

/// A drawing dimension (width, height, radius, tile count).
///
/// Implemented for `i32`, `u32` and `NonZeroU32`, so a draw call accepts a
/// literal, a value computed from positions, or an already-validated size.
pub trait Dim: sealed::Sealed {
    /// The dimension as a strictly positive value, or `None` if it was zero or
    /// negative.
    fn to_nonzero(self) -> Option<NonZeroU32>;
}

impl sealed::Sealed for i32 {}
impl Dim for i32 {
    fn to_nonzero(self) -> Option<NonZeroU32> {
        u32::try_from(self).ok().and_then(NonZeroU32::new)
    }
}

impl sealed::Sealed for u32 {}
impl Dim for u32 {
    fn to_nonzero(self) -> Option<NonZeroU32> {
        NonZeroU32::new(self)
    }
}

impl sealed::Sealed for NonZeroU32 {}
impl Dim for NonZeroU32 {
    fn to_nonzero(self) -> Option<NonZeroU32> {
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
    use core::num::NonZeroU32;

    #[test]
    fn i32_conversions() {
        assert_eq!(5i32.to_nonzero(), NonZeroU32::new(5));
        assert_eq!(0i32.to_nonzero(), None);
        assert_eq!((-3i32).to_nonzero(), None);
    }

    #[test]
    fn u32_conversions() {
        assert_eq!(5u32.to_nonzero(), NonZeroU32::new(5));
        assert_eq!(0u32.to_nonzero(), None);
    }

    #[test]
    fn nonzero_passes_through() {
        let n = NonZeroU32::new(7).unwrap();
        assert_eq!(n.to_nonzero(), Some(n));
    }
}
