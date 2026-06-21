//! The music playback API.

use crate::flags::bitflag_enum;

bitflag_enum! {
    /// One of the four audio channels. Reserve channels for music with
    /// `Music::reserve_channels`.
    pub enum MusicChannel {
        Channel0 = 1 << 0,
        Channel1 = 1 << 1,
        Channel2 = 1 << 2,
        Channel3 = 1 << 3,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::BitFlags;

    #[test]
    fn channel_bits_and_combine() {
        assert_eq!(MusicChannel::Channel0 as u8, 0b0001);
        assert_eq!(MusicChannel::Channel3 as u8, 0b1000);
        let set = MusicChannel::Channel0 | MusicChannel::Channel2;
        assert_eq!(set.bits(), 0b0101);
        let one: BitFlags<MusicChannel> = MusicChannel::Channel1.into();
        assert_eq!(one.bits(), 0b0010);
    }
}
