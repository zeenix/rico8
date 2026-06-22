//! A tiny `u8`-backed bit-flags abstraction, so the SDK stays dependency-free.
//!
//! Only the behaviour the SDK needs: build a set from raw bits (rejecting unknown bits),
//! test membership, and combine flags with `|`. Both flag spaces in the console —
//! [`Button`](crate::Button) and [`SpriteFlag`](crate::SpriteFlag) — fit in a `u8`, so the
//! backing integer is fixed at `u8` by design.

use core::{marker::PhantomData, ops::BitOr};

/// A single bit flag backed by a `u8` mask.
pub trait BitFlag: Copy {
    /// Bitwise-OR of every flag — the bits [`BitFlags::from_bits`] accepts.
    const ALL_BITS: u8;

    /// This flag's single-bit value.
    ///
    /// Implementations must return exactly one set bit, and that bit must be
    /// part of [`ALL_BITS`](Self::ALL_BITS). [`From`] and [`BitOr`] rely on
    /// this so a single flag always converts to a valid one-element set.
    fn bits(self) -> u8;
}

/// A set of [`BitFlag`]s of type `T`.
pub struct BitFlags<T: BitFlag> {
    bits: u8,
    _marker: PhantomData<T>,
}

impl<T> BitFlags<T>
where
    T: BitFlag,
{
    /// The empty set.
    pub const fn empty() -> Self {
        Self {
            bits: 0,
            _marker: PhantomData,
        }
    }

    /// A set from raw bits, without checking they are valid flags of `T`.
    ///
    /// `const`, for predefined combinations known at compile time (e.g. the
    /// diagonal button pairs). Prefer [`from_bits`](Self::from_bits) whenever
    /// the bits are not statically known to be valid.
    ///
    /// # Safety
    ///
    /// `bits` must contain only valid flag bits of `T`, that is
    /// `bits & !T::ALL_BITS == 0`. The check is skipped, so any other bit
    /// yields a set whose bits do not all name a real flag — the invariant the
    /// rest of the type is written against.
    pub(crate) const unsafe fn from_bits_unchecked(bits: u8) -> Self {
        Self {
            bits,
            _marker: PhantomData,
        }
    }

    /// A set from raw bits, or [`UnknownBits`] if any bit is not a valid flag.
    ///
    /// An unrecognized bit is reported rather than silently dropped: it means
    /// the raw value carried a flag this build does not know, which usually
    /// signals an ABI mismatch between the host and the cart.
    pub fn from_bits(bits: u8) -> Result<Self, UnknownBits> {
        let unknown = bits & !T::ALL_BITS;
        if unknown != 0 {
            return Err(UnknownBits { bits: unknown });
        }
        Ok(Self {
            bits,
            _marker: PhantomData,
        })
    }

    /// The raw bits.
    pub const fn bits(self) -> u8 {
        self.bits
    }

    /// Whether no flags are set.
    pub fn is_empty(self) -> bool {
        self.bits == 0
    }

    /// Whether every flag in `other` is also set here.
    pub fn contains(self, other: impl Into<BitFlags<T>>) -> bool {
        let other = other.into().bits;
        self.bits & other == other
    }
}

/// The error from [`BitFlags::from_bits`]: the raw value had bits set that do
/// not correspond to any flag.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct UnknownBits {
    /// The unrecognized bits — those outside the flag set.
    pub bits: u8,
}

impl core::fmt::Display for UnknownBits {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "unknown flag bits {:#010b}", self.bits)
    }
}

// `core::error::Error` (stable since Rust 1.81) keeps the impl `no_std`-clean; with
// the `std` feature on, `std::error::Error` is the same trait, so it holds there too.
impl core::error::Error for UnknownBits {}

impl<T> From<T> for BitFlags<T>
where
    T: BitFlag,
{
    fn from(flag: T) -> Self {
        Self {
            bits: flag.bits(),
            _marker: PhantomData,
        }
    }
}

impl<T> BitOr for BitFlags<T>
where
    T: BitFlag,
{
    type Output = Self;

    fn bitor(self, rhs: Self) -> Self {
        Self {
            bits: self.bits | rhs.bits,
            _marker: PhantomData,
        }
    }
}

impl<T> BitOr<T> for BitFlags<T>
where
    T: BitFlag,
{
    type Output = Self;

    fn bitor(self, rhs: T) -> Self {
        self | BitFlags::from(rhs)
    }
}

// Hand-written so the impls never require `T` to also implement these traits.
impl<T> Clone for BitFlags<T>
where
    T: BitFlag,
{
    fn clone(&self) -> Self {
        *self
    }
}

impl<T> Copy for BitFlags<T> where T: BitFlag {}

impl<T> PartialEq for BitFlags<T>
where
    T: BitFlag,
{
    fn eq(&self, other: &Self) -> bool {
        self.bits == other.bits
    }
}

impl<T> Eq for BitFlags<T> where T: BitFlag {}

impl<T> core::fmt::Debug for BitFlags<T>
where
    T: BitFlag,
{
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "BitFlags({:#010b})", self.bits)
    }
}

/// Defines a `#[repr(u8)]` flag enum and its [`BitFlag`] impl, computing `ALL_BITS` from the
/// variant list so the truncation mask can never drift from the variants. Replaces `#[bitflags]`.
macro_rules! bitflag_enum {
    (
        $(#[$meta:meta])*
        $vis:vis enum $name:ident {
            $( $(#[$vmeta:meta])* $variant:ident = $value:expr ),+ $(,)?
        }
    ) => {
        $(#[$meta])*
        #[repr(u8)]
        #[derive(Debug, Clone, Copy, PartialEq, Eq)]
        $vis enum $name {
            $( $(#[$vmeta])* $variant = $value ),+
        }

        impl $crate::flags::BitFlag for $name {
            const ALL_BITS: u8 = 0 $( | ($name::$variant as u8) )+;

            fn bits(self) -> u8 {
                self as u8
            }
        }

        impl ::core::ops::BitOr for $name {
            type Output = $crate::flags::BitFlags<$name>;

            fn bitor(self, rhs: Self) -> Self::Output {
                $crate::flags::BitFlags::from(self) | rhs
            }
        }
    };
}
pub(crate) use bitflag_enum;

#[cfg(test)]
mod tests {
    use super::*;

    bitflag_enum! {
        /// Flags used only by these tests.
        pub enum Test {
            A = 1 << 0,
            B = 1 << 1,
            C = 1 << 2,
        }
    }

    #[test]
    fn all_bits_is_or_of_variants() {
        assert_eq!(Test::ALL_BITS, 0b0000_0111);
    }

    #[test]
    fn from_bits_validates_bits() {
        // Bit 3 (0b1000) is not a flag, so it is reported, not silently dropped.
        assert_eq!(
            BitFlags::<Test>::from_bits(0b1101),
            Err(UnknownBits { bits: 0b1000 }),
        );
        // Only-valid bits round-trip; the empty value is accepted too.
        let set = BitFlags::<Test>::from_bits(0b0101).unwrap();
        assert_eq!(set.bits(), 0b0101);
        assert!(set.contains(Test::A));
        assert!(set.contains(Test::C));
        assert!(!set.contains(Test::B));
        assert!(BitFlags::<Test>::from_bits(0).unwrap().is_empty());
    }

    #[test]
    fn empty_set_is_empty() {
        let set = BitFlags::<Test>::empty();
        assert!(set.is_empty());
        assert_eq!(set.bits(), 0);
        assert!(!set.contains(Test::A));
    }

    #[test]
    fn from_single_flag() {
        let set: BitFlags<Test> = Test::B.into();
        assert_eq!(set.bits(), 0b0010);
        assert!(!set.is_empty());
    }

    #[test]
    fn combine_with_bitor() {
        let set = Test::A | Test::C; // flag | flag
        assert!(set.contains(Test::A));
        assert!(set.contains(Test::C));
        assert!(!set.contains(Test::B));

        let more = set | Test::B; // set | flag
        assert_eq!(more.bits(), Test::ALL_BITS);

        let union = (Test::A | Test::B) | (Test::B | Test::C); // set | set
        assert_eq!(union.bits(), Test::ALL_BITS);
    }

    #[test]
    fn contains_superset_semantics() {
        let set = Test::A | Test::B;
        assert!(set.contains(Test::A | Test::B));
        assert!(!set.contains(Test::A | Test::C));
    }

    #[test]
    fn unknown_bits_is_an_error() {
        // It satisfies the standard `Error` bound (`core::error::Error`, so this
        // holds with or without the `std` feature).
        fn assert_error<E>()
        where
            E: core::error::Error,
        {
        }
        assert_error::<UnknownBits>();

        // Its `Display` names the offending bits.
        use core::fmt::Write as _;
        let mut buf = crate::fmt::FmtBuf::<64>::new();
        write!(buf, "{}", UnknownBits { bits: 0b1000 }).unwrap();
        assert_eq!(buf.as_str(), "unknown flag bits 0b00001000");
    }
}
