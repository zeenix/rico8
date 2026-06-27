//! The music playback API.

use crate::{ffi, flags::bitflag_enum, BitFlags, MusicId};

bitflag_enum! {
    /// One of the four audio channels. Reserve channels for music with
    /// [`Music::reserve_channels`].
    pub enum MusicChannel {
        Channel0 = 1 << 0,
        Channel1 = 1 << 1,
        Channel2 = 1 << 2,
        Channel3 = 1 << 3,
    }
}

/// A configured-but-not-yet-playing music request.
///
/// Build it with [`Context::music`](crate::Context::music), optionally set a
/// fade-in and reserved channels, then [`play`](Self::play).
#[must_use = "music does nothing until you call .play()"]
#[derive(Debug, Clone, Copy)]
pub struct Music {
    pattern: MusicId,
    fade_in_duration: u32,
    channels: BitFlags<MusicChannel>,
}

impl Music {
    pub(crate) fn new(pattern: MusicId) -> Self {
        Self {
            pattern,
            fade_in_duration: 0,
            channels: BitFlags::empty(),
        }
    }

    /// Fade the music in over `duration` milliseconds instead of starting at
    /// full volume.
    pub fn fade_in(mut self, duration: u32) -> Self {
        self.fade_in_duration = duration;
        self
    }

    /// Reserve these channels for music: auto-routed [`Context::sfx`] calls will
    /// avoid them while the music plays.
    ///
    /// [`Context::sfx`]: crate::Context::sfx
    pub fn reserve_channels(mut self, channels: impl Into<BitFlags<MusicChannel>>) -> Self {
        self.channels = channels.into();
        self
    }

    /// Start playing.
    ///
    /// On success returns a [`PlayingMusic`] handle that controls — and, when
    /// dropped, stops — this song. Fails with [`MusicBusy`] if a song is already
    /// playing; stop it first, then retry.
    pub fn play(self) -> Result<PlayingMusic, MusicBusy> {
        let token = unsafe {
            ffi::music(
                self.pattern.0 as i32,
                self.fade_in_duration as i32,
                self.channels.bits() as i32,
                0,
            )
        };
        if token != 0 {
            Ok(PlayingMusic {
                token,
                fade_out_duration: 0,
            })
        } else {
            Err(MusicBusy(self))
        }
    }
}

/// The error from [`Music::play`]: a song is already playing, so the request was
/// refused. Carries the rejected [`Music`] so it can be retried after the
/// current song stops, without rebuilding.
#[derive(Debug, Clone, Copy)]
pub struct MusicBusy(pub Music);

impl core::fmt::Display for MusicBusy {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str("a song is already playing")
    }
}

// `core::error::Error` (stable since Rust 1.81) keeps the impl `no_std`-clean; with
// the `std` feature on, `std::error::Error` is the same trait, so it holds there too.
impl core::error::Error for MusicBusy {}

/// A handle to the currently-playing song.
///
/// Dropping it stops that song (fading out first if [`fade_out`](Self::fade_out)
/// armed a fade). The handle is tied to the song it started: once that song ends
/// or is replaced, its stop becomes a harmless no-op.
// Deliberately not `Clone`/`Copy`: the handle owns the running song, and the
// drop-stops-the-song contract relies on there being exactly one of it.
#[derive(Debug)]
#[must_use = "dropping the handle stops the music; keep it to control playback"]
pub struct PlayingMusic {
    token: i32,
    fade_out_duration: u32,
}

impl PlayingMusic {
    /// Arm a fade-out: when this handle is stopped or dropped, the song fades to
    /// silence over `duration` milliseconds instead of cutting out.
    pub fn fade_out(mut self, duration: u32) -> Self {
        self.fade_out_duration = duration;
        self
    }

    /// Stop the song now (fading out over the armed duration, if any).
    pub fn stop(self) {
        // Consuming `self` runs `Drop`, which issues the stop.
    }
}

impl Drop for PlayingMusic {
    fn drop(&mut self) {
        unsafe { ffi::music(-1, self.fade_out_duration as i32, 0, self.token) };
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

    #[test]
    fn play_returns_a_handle_on_the_native_stub() {
        // On the host target `ffi::music` is a stub that returns a nonzero token,
        // so a configured request plays and yields a storable handle.
        let m = Music::new(crate::MusicId::new(0).unwrap())
            .fade_in(500)
            .reserve_channels(MusicChannel::Channel0 | MusicChannel::Channel1);
        let handle: Option<PlayingMusic> = m.play().ok();
        assert!(handle.is_some(), "stub start succeeds");
        // The handle can be armed for a fade-out and stopped.
        handle.unwrap().fade_out(500).stop();
    }

    #[test]
    fn music_busy_is_an_error() {
        // It satisfies the standard `Error` bound (`core::error::Error`, so this
        // holds with or without the `std` feature).
        fn assert_error<E>()
        where
            E: core::error::Error,
        {
        }
        assert_error::<MusicBusy>();

        // Its `Display` explains the refusal.
        use core::fmt::Write as _;
        let mut buf = crate::fmt::FmtBuf::<64>::new();
        write!(buf, "{}", MusicBusy(Music::new(MusicId::new(0).unwrap()))).unwrap();
        assert_eq!(buf.as_str(), "a song is already playing");
    }
}
