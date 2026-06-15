//! Audio runtime: a 4-channel chip-tune synthesizer.
//!
//! The synth core is pure (samples in, samples out) so it can be tested
//! headless; `AudioOutput` hooks it to a real device via cpal when the
//! `audio` feature is enabled and an output device exists. On machines
//! with no audio device RICO-8 stays silent but fully functional.

use crate::assets::{MusicPattern, Sfx, SfxEffect, Waveform, CHANNELS, SFX_LEN};
use std::sync::{Arc, Mutex};

/// Steps are timed in 1/128ths of a second, like the SFX `speed` field.
const TICKS_PER_SECOND: f32 = 128.0;

fn pitch_to_freq(pitch: f32) -> f32 {
    // Pitch 33 = A-4 = 440 Hz, 12 steps per octave.
    440.0 * ((pitch - 33.0) / 12.0).exp2()
}

/// True when the SFX loops (a real loop range, not a LEN marker).
fn sfx_loops(sfx: &Sfx) -> bool {
    sfx.loop_end > sfx.loop_start
}

/// Steps the SFX occupies for music timing: its loop end when looping, its
/// LEN marker (`loop_start` with no loop end), otherwise the full 32.
fn sfx_steps(sfx: &Sfx) -> usize {
    if sfx.loop_end > sfx.loop_start {
        sfx.loop_end as usize
    } else if sfx.loop_start > 0 {
        sfx.loop_start as usize
    } else {
        SFX_LEN
    }
}

/// One play-through of the SFX in seconds, used to time music patterns.
fn sfx_duration(sfx: &Sfx) -> f32 {
    sfx_steps(sfx) as f32 * sfx.speed.max(1) as f32 / TICKS_PER_SECOND
}

/// One sample of a deterministic (non-noise) waveform at `phase` in `[0, 1)`.
/// Factored out so the `detune` filter can run a second oscillator through it.
fn tonal_wave(wave: Waveform, phase: f32) -> f32 {
    match wave {
        Waveform::Triangle => 4.0 * (phase - 0.5).abs() - 1.0,
        Waveform::TiltedSaw => {
            if phase < 0.875 {
                phase / 0.875 * 2.0 - 1.0
            } else {
                (1.0 - phase) / 0.125 * 2.0 - 1.0
            }
        }
        Waveform::Saw => 2.0 * phase - 1.0,
        Waveform::Square => {
            if phase < 0.5 {
                1.0
            } else {
                -1.0
            }
        }
        Waveform::Pulse => {
            if phase < 0.3125 {
                1.0
            } else {
                -1.0
            }
        }
        Waveform::Organ => {
            let t1 = 4.0 * (phase - 0.5).abs() - 1.0;
            let p2 = (phase * 0.5).fract();
            let t2 = 4.0 * (p2 - 0.5).abs() - 1.0;
            (t1 + t2) * 0.5
        }
        Waveform::Phaser => {
            let t1 = 4.0 * (phase - 0.5).abs() - 1.0;
            let p2 = (phase * 1.01).fract();
            let t2 = 4.0 * (p2 - 0.5).abs() - 1.0;
            (t1 + t2) * 0.5
        }
        // Noise is stateful; handled directly in `Voice::sample`.
        Waveform::Noise => 0.0,
    }
}

/// One playing voice on a channel.
struct Voice {
    sfx_index: usize,
    sfx: Sfx,
    /// Current step in `0..SFX_LEN`.
    step: usize,
    /// Seconds elapsed within the current step.
    t_in_step: f32,
    /// Oscillator phase in `[0, 1)`.
    phase: f32,
    /// Phase of the detuned second oscillator (`detune` filter).
    phase2: f32,
    /// Pitch of the previous step, for slides.
    prev_pitch: f32,
    /// True when this voice was started by the music sequencer.
    from_music: bool,
    /// Noise generator state.
    noise: u32,
    noise_level: f32,
    /// One-pole low-pass state (`dampen` filter).
    lp: f32,
    /// Echo delay ring buffer and write cursor (`reverb` filter); empty when
    /// reverb is off.
    echo: Vec<f32>,
    echo_pos: usize,
}

impl Voice {
    fn new(sfx_index: usize, sfx: Sfx, from_music: bool, sample_rate: f32) -> Self {
        let first_pitch = sfx.notes[0].pitch as f32;
        // Reverb delays by 2 or 4 ticks; size the ring buffer to suit.
        let echo_ticks = match sfx.reverb {
            1 => 2.0,
            2 => 4.0,
            _ => 0.0,
        };
        let echo_len = (echo_ticks / TICKS_PER_SECOND * sample_rate).round() as usize;
        Self {
            sfx_index,
            sfx,
            step: 0,
            t_in_step: 0.0,
            phase: 0.0,
            phase2: 0.0,
            prev_pitch: first_pitch,
            from_music,
            noise: 0x1234_5678,
            noise_level: 0.0,
            lp: 0.0,
            echo: vec![0.0; echo_len],
            echo_pos: 0,
        }
    }

    fn step_duration(&self) -> f32 {
        self.sfx.speed.max(1) as f32 / TICKS_PER_SECOND
    }

    /// Render one sample; returns `None` when the voice has finished.
    ///
    /// `inst_waves` carries the timbre of each of the eight SFX slots usable
    /// as custom instruments (its note-0 waveform), so a note flagged as a
    /// custom instrument plays through that waveform at its own pitch.
    fn sample(&mut self, dt: f32, total_t: f32, inst_waves: &[u8; 8]) -> Option<f32> {
        if self.step >= SFX_LEN {
            return None;
        }
        let note = self.sfx.notes[self.step];
        let frac = self.t_in_step / self.step_duration();

        // Resolve effect-modified pitch and volume.
        let base_pitch = note.pitch as f32;
        let mut pitch = base_pitch;
        let mut vol = note.volume as f32 / 7.0;
        match SfxEffect::from_u8(note.effect) {
            SfxEffect::None => {}
            SfxEffect::Slide => pitch = self.prev_pitch + (base_pitch - self.prev_pitch) * frac,
            SfxEffect::Vibrato => {
                pitch += 0.25 * (total_t * 2.0 * std::f32::consts::PI * 8.0).sin()
            }
            SfxEffect::Drop => pitch = base_pitch * (1.0 - frac),
            SfxEffect::FadeIn => vol *= frac,
            SfxEffect::FadeOut => vol *= 1.0 - frac,
            SfxEffect::ArpFast | SfxEffect::ArpSlow => {
                let rate = if note.effect == 6 { 32.0 } else { 16.0 };
                let group = self.step / 4 * 4;
                let idx = (total_t * rate) as usize % 4;
                pitch = self.sfx.notes[(group + idx).min(SFX_LEN - 1)].pitch as f32;
            }
        }

        let freq = pitch_to_freq(pitch);
        // A custom-instrument note borrows the timbre of another SFX (its
        // note-0 waveform); otherwise the nibble names a built-in waveform.
        let wave = match note.instrument() {
            Some(slot) => Waveform::from_u8(inst_waves[slot as usize]),
            None => Waveform::from_u8(note.wave),
        };

        // Advance oscillator.
        self.phase = (self.phase + freq * dt).fract();
        let mut raw = if wave == Waveform::Noise {
            if self.sfx.noiz {
                // `noiz` swaps the pitched noise for pure white noise.
                self.noise = self.noise.wrapping_mul(1664525).wrapping_add(1013904223);
                (self.noise >> 16) as f32 / 32768.0 - 1.0
            } else {
                // Resample an LFSR at the note frequency for pitched noise.
                if self.phase < freq * dt {
                    self.noise = self.noise.wrapping_mul(1664525).wrapping_add(1013904223);
                    self.noise_level = (self.noise >> 16) as f32 / 32768.0 - 1.0;
                }
                self.noise_level
            }
        } else {
            let mut s = tonal_wave(wave, self.phase);
            // `detune` mixes in a second oscillator a little (or an octave)
            // off the first.
            if self.sfx.detune > 0 {
                let ratio = if self.sfx.detune == 1 { 1.0073 } else { 2.0 };
                self.phase2 = (self.phase2 + freq * ratio * dt).fract();
                s = (s + tonal_wave(wave, self.phase2)) * 0.5;
            }
            s
        };

        // `buzz` adds harmonics with a soft overdrive (normalized to unity).
        if self.sfx.buzz {
            const DRIVE: f32 = 2.5;
            raw = (raw * DRIVE).tanh() / DRIVE.tanh();
        }

        let mut out = raw * vol * 0.25;

        // `dampen` is a one-pole low-pass at one of two cutoffs.
        if self.sfx.dampen > 0 {
            let fc = if self.sfx.dampen == 1 { 2200.0 } else { 900.0 };
            let rc = 1.0 / (2.0 * std::f32::consts::PI * fc);
            let alpha = dt / (rc + dt);
            self.lp += alpha * (out - self.lp);
            out = self.lp;
        }

        // `reverb` is a feedback echo through the delay ring buffer.
        if !self.echo.is_empty() {
            let delayed = self.echo[self.echo_pos];
            self.echo[self.echo_pos] = (out + delayed * 0.45).clamp(-1.0, 1.0);
            self.echo_pos = (self.echo_pos + 1) % self.echo.len();
            out = (out + delayed * 0.5).clamp(-1.0, 1.0);
        }

        // Advance step clock.
        self.t_in_step += dt;
        if self.t_in_step >= self.step_duration() {
            self.t_in_step = 0.0;
            self.prev_pitch = base_pitch;
            self.step += 1;
            let (ls, le) = (self.sfx.loop_start as usize, self.sfx.loop_end as usize);
            if le > ls {
                // Looping SFX wrap at the loop end — for music voices too, so
                // a short looping part repeats to fill its pattern (the
                // sequencer replaces the voice when the pattern advances).
                if self.step >= le {
                    self.step = ls;
                }
            } else if ls > 0 && self.step >= ls {
                // A "LEN" marker (loop start set, no loop end) shortens the
                // SFX to `loop_start` steps.
                self.step = SFX_LEN;
            }
        }
        Some(out)
    }
}

/// Music sequencer state.
struct MusicState {
    pattern: usize,
    /// Seconds remaining in the current pattern.
    remaining: f32,
}

/// The synthesizer: voices, sequencer and a copy of the cart's audio data.
pub struct Synth {
    sample_rate: f32,
    t: f32,
    sfx: Vec<Sfx>,
    music: Vec<MusicPattern>,
    voices: [Option<Voice>; CHANNELS],
    music_state: Option<MusicState>,
}

impl Synth {
    pub fn new(sample_rate: f32) -> Self {
        Self {
            sample_rate,
            t: 0.0,
            sfx: Vec::new(),
            music: Vec::new(),
            voices: [None, None, None, None],
            music_state: None,
        }
    }

    /// Replace the audio data (called when a cart starts or assets change).
    pub fn load(&mut self, sfx: Vec<Sfx>, music: Vec<MusicPattern>) {
        self.sfx = sfx;
        self.music = music;
    }

    /// Stop all voices and the sequencer.
    pub fn stop_all(&mut self) {
        self.voices = [None, None, None, None];
        self.music_state = None;
    }

    /// Play SFX `n`. `channel < 0` picks a free channel (preferring ones not
    /// used by music); `n < 0` with a valid channel stops that channel.
    pub fn play_sfx(&mut self, n: i32, channel: i32) {
        if n < 0 {
            if (0..CHANNELS as i32).contains(&channel) {
                self.voices[channel as usize] = None;
            }
            return;
        }
        let Some(sfx) = self.sfx.get(n as usize).cloned() else {
            return;
        };
        let ch = if (0..CHANNELS as i32).contains(&channel) {
            channel as usize
        } else {
            // Prefer an idle channel, then one playing a one-shot SFX;
            // steal a music channel only as a last resort.
            let idle = (0..CHANNELS).find(|&i| self.voices[i].is_none());
            let non_music =
                (0..CHANNELS).find(|&i| self.voices[i].as_ref().is_some_and(|v| !v.from_music));
            match idle.or(non_music) {
                Some(i) => i,
                None => CHANNELS - 1,
            }
        };
        self.voices[ch] = Some(Voice::new(n as usize, sfx, false, self.sample_rate));
    }

    /// Start music at pattern `n`, or stop when `n < 0`.
    pub fn play_music(&mut self, n: i32) {
        if n < 0 {
            self.stop_music();
            return;
        }
        self.start_pattern(n as usize);
    }

    pub fn stop_music(&mut self) {
        for v in &mut self.voices {
            if v.as_ref().is_some_and(|v| v.from_music) {
                *v = None;
            }
        }
        self.music_state = None;
    }

    /// Index of the playing music pattern, if any.
    pub fn playing_pattern(&self) -> Option<usize> {
        self.music_state.as_ref().map(|m| m.pattern)
    }

    fn start_pattern(&mut self, n: usize) {
        let Some(pat) = self.music.get(n).copied() else {
            self.music_state = None;
            return;
        };
        // PICO-8 sets a pattern's length from the left-most non-looping active
        // channel (the "timekeeper"); if every active channel loops, fall back
        // to the longest. SFX shortened by a LEN marker count as that length.
        let mut timekeeper: Option<f32> = None;
        let mut longest = 0.0f32;
        for (ch, slot) in pat.channels.iter().enumerate() {
            // Music takes ownership of its channels; others keep playing SFX.
            if let Some(sfx_idx) = slot {
                if let Some(sfx) = self.sfx.get(*sfx_idx as usize).cloned() {
                    let dur = sfx_duration(&sfx);
                    longest = longest.max(dur);
                    if timekeeper.is_none() && !sfx_loops(&sfx) {
                        timekeeper = Some(dur);
                    }
                    self.voices[ch] =
                        Some(Voice::new(*sfx_idx as usize, sfx, true, self.sample_rate));
                }
            } else if self.voices[ch].as_ref().is_some_and(|v| v.from_music) {
                self.voices[ch] = None;
            }
        }
        let length = timekeeper.unwrap_or(longest);
        if length == 0.0 {
            self.music_state = None;
            return;
        }
        self.music_state = Some(MusicState {
            pattern: n,
            remaining: length,
        });
    }

    fn advance_music(&mut self) {
        let Some(state) = &self.music_state else {
            return;
        };
        let cur = state.pattern;
        let pat = self.music.get(cur).copied().unwrap_or_default();
        if pat.stop_at_end {
            self.stop_music();
            return;
        }
        if pat.loop_back {
            // Jump back to the nearest loop_start at or before this pattern.
            let target = (0..=cur)
                .rev()
                .find(|&i| self.music.get(i).is_some_and(|p| p.loop_start))
                .unwrap_or(0);
            self.start_pattern(target);
            return;
        }
        let next = cur + 1;
        if self.music.get(next).is_some_and(|p| !p.is_empty()) {
            self.start_pattern(next);
        } else {
            self.stop_music();
        }
    }

    /// Render one mono sample.
    pub fn next_sample(&mut self) -> f32 {
        let dt = 1.0 / self.sample_rate;
        self.t += dt;

        if let Some(state) = &mut self.music_state {
            state.remaining -= dt;
            if state.remaining <= 0.0 {
                self.advance_music();
            }
        }

        // Timbre of the eight SFX slots usable as custom instruments.
        let mut inst_waves = [0u8; 8];
        for (i, w) in inst_waves.iter_mut().enumerate() {
            if let Some(s) = self.sfx.get(i) {
                *w = s.notes[0].wave_index();
            }
        }

        let mut mix = 0.0;
        for v in &mut self.voices {
            if let Some(voice) = v {
                match voice.sample(dt, self.t, &inst_waves) {
                    Some(s) => mix += s,
                    None => *v = None,
                }
            }
        }
        mix.clamp(-1.0, 1.0)
    }

    /// Which SFX index is playing on each channel (for editor UI).
    pub fn channel_sfx(&self) -> [Option<usize>; CHANNELS] {
        let mut out = [None; CHANNELS];
        for (i, v) in self.voices.iter().enumerate() {
            out[i] = v.as_ref().map(|v| v.sfx_index);
        }
        out
    }

    /// Which step each channel's voice is currently sounding (for editor
    /// playheads); `None` when the channel is idle.
    pub fn channel_step(&self) -> [Option<usize>; CHANNELS] {
        let mut out = [None; CHANNELS];
        for (i, v) in self.voices.iter().enumerate() {
            out[i] = v.as_ref().map(|v| v.step);
        }
        out
    }
}

/// Clonable handle the VM and editors use to poke the synth.
#[derive(Clone)]
pub struct AudioHandle {
    synth: Arc<Mutex<Synth>>,
}

impl AudioHandle {
    pub fn new(synth: Arc<Mutex<Synth>>) -> Self {
        Self { synth }
    }

    /// A handle with no device attached — still fully functional for logic.
    pub fn dummy() -> Self {
        Self {
            synth: Arc::new(Mutex::new(Synth::new(44100.0))),
        }
    }

    pub fn with_synth<R>(&self, f: impl FnOnce(&mut Synth) -> R) -> R {
        // Recover from a poisoned lock instead of cascading the panic:
        // a one-off hiccup in the audio callback shouldn't permanently
        // silence the synth or take down the next caller.
        let mut guard = self.synth.lock().unwrap_or_else(|e| e.into_inner());
        f(&mut guard)
    }

    pub fn play_sfx(&self, n: i32, channel: i32) {
        self.with_synth(|s| s.play_sfx(n, channel));
    }

    /// The step each channel's voice is sounding (for editor playheads).
    pub fn channel_step(&self) -> [Option<usize>; CHANNELS] {
        self.with_synth(|s| s.channel_step())
    }

    pub fn play_music(&self, n: i32) {
        self.with_synth(|s| s.play_music(n));
    }

    pub fn stop_all(&self) {
        self.with_synth(|s| s.stop_all());
    }

    pub fn load(&self, sfx: Vec<Sfx>, music: Vec<MusicPattern>) {
        self.with_synth(|s| s.load(sfx, music));
    }
}

/// Real audio output via cpal. Owns the stream; dropping it stops audio.
#[cfg(feature = "audio")]
pub struct AudioOutput {
    _stream: cpal::Stream,
    handle: AudioHandle,
}

#[cfg(feature = "audio")]
impl AudioOutput {
    /// Try to open the default output device. Returns `None` (silently)
    /// when no device is available, e.g. on headless machines.
    pub fn start() -> Option<Self> {
        use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
        let host = cpal::default_host();
        let device = host.default_output_device()?;
        let config = device.default_output_config().ok()?;
        let sample_rate = config.sample_rate() as f32;
        let channels = config.channels() as usize;
        let synth = Arc::new(Mutex::new(Synth::new(sample_rate)));
        let cb_synth = synth.clone();
        let stream = device
            .build_output_stream(
                config.into(),
                move |data: &mut [f32], _| {
                    let mut synth = cb_synth.lock().unwrap();
                    for frame in data.chunks_mut(channels) {
                        let s = synth.next_sample();
                        for out in frame {
                            *out = s;
                        }
                    }
                },
                |err| eprintln!("rico8 audio error: {err}"),
                None,
            )
            .ok()?;
        stream.play().ok()?;
        Some(Self {
            _stream: stream,
            handle: AudioHandle::new(synth),
        })
    }

    pub fn handle(&self) -> AudioHandle {
        self.handle.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::assets::{Note, SFX_COUNT};

    fn test_sfx() -> Vec<Sfx> {
        let mut sfx = vec![Sfx::default(); SFX_COUNT];
        for note in sfx[0].notes.iter_mut() {
            *note = Note {
                pitch: 33,
                wave: 0,
                volume: 5,
                effect: 0,
            };
        }
        sfx
    }

    #[test]
    fn pitch_33_is_a440() {
        assert!((pitch_to_freq(33.0) - 440.0).abs() < 0.01);
        assert!((pitch_to_freq(45.0) - 880.0).abs() < 0.01);
    }

    #[test]
    fn sfx_produces_sound_then_ends() {
        let mut synth = Synth::new(44100.0);
        synth.load(test_sfx(), vec![MusicPattern::default(); 64]);
        synth.play_sfx(0, 0);
        let mut peak = 0.0f32;
        for _ in 0..1000 {
            peak = peak.max(synth.next_sample().abs());
        }
        assert!(peak > 0.01, "voice should be audible");
        // Default speed 16 -> 32 steps * 0.125 s = 4 s; play 5 s.
        for _ in 0..(44100 * 5) {
            synth.next_sample();
        }
        assert_eq!(synth.channel_sfx()[0], None, "voice should end");
    }

    #[test]
    fn custom_instrument_borrows_its_waveform() {
        use crate::assets::NOTE_CUSTOM_FLAG;
        // SFX 1 is the instrument: a noise (waveform 6) tone.
        let mut sfx = vec![Sfx::default(); SFX_COUNT];
        for note in sfx[1].notes.iter_mut() {
            *note = Note {
                pitch: 33,
                wave: 6,
                volume: 5,
                effect: 0,
            };
        }
        // SFX 0 plays using SFX 1 as a custom instrument.
        for note in sfx[0].notes.iter_mut() {
            *note = Note {
                pitch: 33,
                wave: NOTE_CUSTOM_FLAG | 1,
                volume: 5,
                effect: 0,
            };
        }
        let mut synth = Synth::new(44100.0);
        synth.load(sfx, vec![MusicPattern::default(); 64]);
        synth.play_sfx(0, 0);
        let mut peak = 0.0f32;
        for _ in 0..1000 {
            peak = peak.max(synth.next_sample().abs());
        }
        assert!(peak > 0.01, "a custom-instrument note should be audible");
    }

    #[test]
    fn sfx_filters_stay_audible_and_bounded() {
        // Every filter switch on at once must still produce a clean, bounded
        // signal (no NaNs, no runaway feedback).
        let mut sfx = test_sfx();
        sfx[0].noiz = true;
        sfx[0].buzz = true;
        sfx[0].detune = 2;
        sfx[0].reverb = 2;
        sfx[0].dampen = 1;
        let mut synth = Synth::new(44100.0);
        synth.load(sfx, vec![MusicPattern::default(); 64]);
        synth.play_sfx(0, 0);
        let mut peak = 0.0f32;
        for _ in 0..44100 {
            let s = synth.next_sample();
            assert!(s.is_finite() && s.abs() <= 1.0, "sample out of range: {s}");
            peak = peak.max(s.abs());
        }
        assert!(peak > 0.01, "filtered voice should still be audible");
    }

    #[test]
    fn empty_sfx_slot_is_ignored() {
        let mut synth = Synth::new(44100.0);
        synth.load(test_sfx(), vec![]);
        synth.play_sfx(63, -1);
        for _ in 0..100 {
            assert_eq!(synth.next_sample(), 0.0);
        }
    }

    #[test]
    fn music_plays_and_stops() {
        let mut synth = Synth::new(44100.0);
        let mut music = vec![MusicPattern::default(); 64];
        music[0].channels[0] = Some(0);
        music[0].stop_at_end = true;
        synth.load(test_sfx(), music);
        synth.play_music(0);
        assert_eq!(synth.playing_pattern(), Some(0));
        for _ in 0..(44100 * 5) {
            synth.next_sample();
        }
        assert_eq!(synth.playing_pattern(), None);
    }

    #[test]
    fn music_loops_back() {
        let mut synth = Synth::new(44100.0);
        let mut music = vec![MusicPattern::default(); 64];
        music[0].channels[0] = Some(0);
        music[0].loop_start = true;
        music[1].channels[0] = Some(0);
        music[1].loop_back = true;
        synth.load(test_sfx(), music);
        synth.play_music(1);
        for _ in 0..(44100 * 5) {
            synth.next_sample();
        }
        assert_eq!(synth.playing_pattern(), Some(0), "should loop to start");
    }

    #[test]
    fn pattern_length_follows_first_non_looping_channel() {
        // ch0 is the timekeeper at speed 4 (1.0s); ch1 is four times longer.
        // The pattern must end with ch0, not stretch to ch1.
        let mut sfx = vec![Sfx::default(); SFX_COUNT];
        for (i, &spd) in [4u8, 16].iter().enumerate() {
            sfx[i].speed = spd;
            for n in sfx[i].notes.iter_mut() {
                *n = Note {
                    pitch: 33,
                    wave: 0,
                    volume: 5,
                    effect: 0,
                };
            }
        }
        let mut music = vec![MusicPattern::default(); 64];
        music[0].channels = [Some(0), Some(1), None, None];
        music[0].stop_at_end = true;
        let mut synth = Synth::new(44100.0);
        synth.load(sfx, music);
        synth.play_music(0);
        let mut n = 0;
        while synth.playing_pattern().is_some() && n < 44100 * 5 {
            synth.next_sample();
            n += 1;
        }
        let secs = n as f32 / 44100.0;
        assert!(
            (secs - 1.0).abs() < 0.1,
            "pattern should track ch0, got {secs}s"
        );
    }

    #[test]
    fn auto_channel_avoids_music() {
        let mut synth = Synth::new(44100.0);
        let mut sfx = test_sfx();
        sfx[1] = sfx[0].clone();
        let mut music = vec![MusicPattern::default(); 64];
        music[0].channels[0] = Some(0);
        synth.load(sfx, music);
        synth.play_music(0);
        synth.play_sfx(1, -1);
        let chans = synth.channel_sfx();
        assert_eq!(chans[0], Some(0), "music keeps channel 0");
        assert!(chans[1..].contains(&Some(1)), "sfx lands elsewhere");
    }

    #[test]
    fn channel_step_tracks_playback() {
        let mut synth = Synth::new(44100.0);
        synth.load(test_sfx(), vec![MusicPattern::default(); 64]);
        assert_eq!(synth.channel_step(), [None, None, None, None]);
        synth.play_sfx(0, 0);
        // After starting, channel 0 is on step 0.
        assert_eq!(synth.channel_step()[0], Some(0));
        // Default speed 16 -> 0.125 s/step; advance ~0.2 s, expect step 1.
        for _ in 0..(44100 / 5) {
            synth.next_sample();
        }
        assert_eq!(synth.channel_step()[0], Some(1));
    }
}
