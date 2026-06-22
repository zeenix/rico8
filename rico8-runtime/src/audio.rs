//! Audio runtime: a 4-channel chip-tune synthesizer.
//!
//! The synth core is pure (samples in, samples out) so it can be tested
//! headless; `AudioOutput` hooks it to a real device via cpal when the
//! `audio` feature is enabled and an output device exists. On machines
//! with no audio device RICO-8 stays silent but fully functional.

use crate::assets::{MusicPattern, Sfx, SfxEffect, Waveform, CHANNELS, SFX_LEN};
use std::sync::{Arc, Mutex};

/// PICO-8 synthesizes at this fixed internal rate; the synth core runs here
/// and `next_sample` resamples up to the device rate.
const INTERNAL_RATE: f32 = 22050.0;

/// PICO-8: one speed-unit tick is 183 samples at the internal rate.
const SAMPLES_PER_TICK: f32 = 183.0;

/// Anti-click: a voice's amplitude ramps toward its target volume at a
/// fixed rate (full 0..1 scale in this many seconds), and starts from zero
/// on onset, matching PICO-8's smooth note-change/onset transitions.
const ANTICLICK_RAMP_SECONDS: f32 = 0.0025;

/// PICO-8's noise low-pass scale (= internal rate / frequency of key 63);
/// the noise cutoff tracks the note frequency through this. (zepto8.)
const NOISE_CUTOFF_SCALE: f32 = 8.858923;

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
    sfx_steps(sfx) as f32 * sfx.speed.max(1) as f32 * SAMPLES_PER_TICK / INTERNAL_RATE
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

/// One sample of a drawn waveform-instrument table at `phase` in `[0, 1)`,
/// linearly interpolated. Samples are signed (`-16..=15`); normalized to
/// roughly `[-1, 1)`.
fn drawn_wave(w: &crate::assets::CustomWave, phase: f32) -> f32 {
    let n = w.samples.len();
    let fpos = phase * n as f32;
    let i0 = (fpos as usize) % n;
    let i1 = (i0 + 1) % n;
    let frac = fpos - fpos.floor();
    let a = w.samples[i0] as f32 / 16.0;
    let b = w.samples[i1] as f32 / 16.0;
    a + (b - a) * frac
}

/// One playing voice on a channel.
struct Voice {
    sfx_index: usize,
    sfx: Sfx,
    /// Current step in `0..SFX_LEN`.
    step: usize,
    /// Seconds elapsed within the current step.
    t_in_step: f32,
    /// Current, slewed output amplitude; ramps toward the note's target
    /// volume to avoid clicks at onsets and note changes (anti-click).
    amp: f32,
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
    fn new(sfx_index: usize, sfx: Sfx, from_music: bool) -> Self {
        let first_pitch = sfx.notes[0].pitch as f32;
        // Reverb delays by 2 or 4 ticks; size the ring buffer to suit. The
        // delay is in internal-sample units (independent of the device rate).
        let echo_ticks = match sfx.reverb {
            1 => 2.0,
            2 => 4.0,
            _ => 0.0,
        };
        let echo_len = (echo_ticks * SAMPLES_PER_TICK).round() as usize;
        Self {
            sfx_index,
            sfx,
            step: 0,
            t_in_step: 0.0,
            // Start silent so the first note ramps up from zero (anti-click).
            amp: 0.0,
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
        self.sfx.speed.max(1) as f32 * SAMPLES_PER_TICK / INTERNAL_RATE
    }

    /// Render one sample; returns `None` when the voice has finished.
    ///
    /// `inst_waves` carries the timbre of each of the eight SFX slots usable
    /// as custom instruments (its note-0 waveform), so a note flagged as a
    /// custom instrument plays through that waveform at its own pitch.
    /// `inst_drawn` carries those slots' drawn waveform tables, when any; a
    /// custom-instrument note whose slot has one plays it instead of a built-in.
    fn sample(
        &mut self,
        dt: f32,
        total_t: f32,
        inst_waves: &[u8; 8],
        inst_drawn: &[Option<crate::assets::CustomWave>; 8],
    ) -> Option<f32> {
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

        // A custom-instrument note borrows the timbre of another SFX: its
        // drawn waveform table when it has one, else that slot's note-0 built-in
        // waveform. A plain note names a built-in waveform directly.
        let drawn = note.instrument().and_then(|slot| inst_drawn[slot as usize]);
        let bass = drawn.is_some_and(|w| w.bass);
        let freq = pitch_to_freq(pitch) * if bass { 0.5 } else { 1.0 };
        let wave = match note.instrument() {
            Some(slot) => Waveform::from_u8(inst_waves[slot as usize]),
            None => Waveform::from_u8(note.wave),
        };

        // Advance oscillator.
        self.phase = (self.phase + freq * dt).fract();
        let mut raw = if let Some(w) = &drawn {
            drawn_wave(w, self.phase)
        } else if wave == Waveform::Noise {
            // PICO-8's noise is a one-pole low-pass of white noise whose cutoff
            // tracks the note frequency (a leaky integrator), so it stays smooth
            // instead of the hard sample-and-hold steps that crackle. (zepto8.)
            self.noise = self.noise.wrapping_mul(1664525).wrapping_add(1013904223);
            let white = (self.noise >> 16) as f32 / 32768.0 - 1.0;
            let scale = freq * dt * NOISE_CUTOFF_SCALE;
            self.noise_level = (self.noise_level + scale * white) / (1.0 + scale);
            let factor = 1.0 - pitch / 63.0;
            let mut n = self.noise_level * 1.5 * (1.0 + factor * factor);
            if self.sfx.noiz {
                // `noiz` brightens the noise: amplitude-modulate by a triangle of
                // the phase.
                n *= 2.0
                    * if self.phase < 0.5 {
                        self.phase
                    } else {
                        self.phase - 1.0
                    };
            }
            n
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
        if self.sfx.buzz && wave != Waveform::Noise {
            const DRIVE: f32 = 2.5;
            raw = (raw * DRIVE).tanh() / DRIVE.tanh();
        }

        // Anti-click: ramp the amplitude toward the target instead of jumping,
        // so note onsets and volume changes between steps don't click.
        let max_step = dt / ANTICLICK_RAMP_SECONDS;
        self.amp += (vol - self.amp).clamp(-max_step, max_step);

        let mut out = raw * self.amp * 0.25;

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
    /// Monotonic counter; each start mints the next play-token.
    token_counter: i32,
    /// The current song's play-token (`0` when nothing is playing).
    current_token: i32,
    /// Gain applied to music voices (`0.0`..=`1.0`), for fades.
    music_gain: f32,
    /// Where `music_gain` is heading.
    music_gain_target: f32,
    /// Per-sample step toward the target (`0.0` once settled).
    music_gain_step: f32,
    /// True while fading out: stop the music when the gain reaches zero.
    stop_when_silent: bool,
    /// Channels reserved for music (bit i = channel i); auto-routed sfx skip them.
    reserved_channels: u8,
    /// Resampler position between `prev_internal` and `cur_internal`.
    resample_frac: f32,
    /// Previous and current internal-rate samples bracketing the output.
    prev_internal: f32,
    cur_internal: f32,
    /// Two cascaded one-pole low-pass states for reconstruction filtering.
    lp1: f32,
    lp2: f32,
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
            token_counter: 0,
            current_token: 0,
            music_gain: 1.0,
            music_gain_target: 1.0,
            music_gain_step: 0.0,
            stop_when_silent: false,
            reserved_channels: 0,
            // Start at 1.0 so the first call renders an internal sample.
            resample_frac: 1.0,
            prev_internal: 0.0,
            cur_internal: 0.0,
            lp1: 0.0,
            lp2: 0.0,
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
        self.current_token = 0;
        self.music_gain = 1.0;
        self.music_gain_target = 1.0;
        self.music_gain_step = 0.0;
        self.stop_when_silent = false;
        self.reserved_channels = 0;
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
            // Prefer an idle non-reserved channel, then one playing a one-shot
            // SFX, then any non-reserved channel; steal a reserved one only when
            // every channel is reserved.
            let reserved = self.reserved_channels;
            let free = |i: usize| reserved & (1 << i) == 0;
            let idle = (0..CHANNELS).find(|&i| self.voices[i].is_none() && free(i));
            let non_music = (0..CHANNELS)
                .find(|&i| free(i) && self.voices[i].as_ref().is_some_and(|v| !v.from_music));
            let any_free = (0..CHANNELS).rev().find(|&i| free(i));
            idle.or(non_music).or(any_free).unwrap_or(CHANNELS - 1)
        };
        self.voices[ch] = Some(Voice::new(n as usize, sfx, false));
    }

    /// Start music at pattern `n` (mints and returns a nonzero play-token) or,
    /// when `n < 0`, stop. A start is refused — returns `0` — while a song is
    /// already playing and not fading out. A stop acts only when `token <= 0`
    /// (unconditional) or `token` equals the current song's play-token.
    /// `channel_mask` bits 0-3 mark which channels are reserved for music;
    /// auto-routed sfx will skip those channels while music is playing.
    pub fn play_music(&mut self, n: i32, fade_duration: i32, channel_mask: i32, token: i32) -> i32 {
        if n < 0 {
            let matches = token <= 0 || (self.current_token != 0 && token == self.current_token);
            if matches {
                self.begin_stop(fade_duration);
            }
            return 0;
        }
        // Refuse a second start only while a song is live (not already fading out).
        if self.music_state.is_some() && !self.stop_when_silent {
            return 0;
        }
        self.reserved_channels = (channel_mask & 0x0F) as u8;
        self.start_pattern(n as usize);
        self.setup_fade_in(fade_duration);
        self.token_counter = self.token_counter.wrapping_add(1);
        if self.token_counter == 0 {
            self.token_counter = 1;
        }
        self.current_token = self.token_counter;
        self.current_token
    }

    /// Arm the fade-in (or instant full volume) for a freshly started song.
    fn setup_fade_in(&mut self, fade_duration: i32) {
        self.stop_when_silent = false;
        if fade_duration <= 0 {
            self.music_gain = 1.0;
            self.music_gain_target = 1.0;
            self.music_gain_step = 0.0;
        } else {
            let fade_seconds = fade_duration as f32 / 1000.0;
            self.music_gain = 0.0;
            self.music_gain_target = 1.0;
            self.music_gain_step = 1.0 / (fade_seconds * INTERNAL_RATE);
        }
    }

    /// Stop now, or ramp to silence over `fade_duration` ms then stop.
    fn begin_stop(&mut self, fade_duration: i32) {
        if self.music_state.is_none() {
            return;
        }
        if fade_duration <= 0 {
            self.stop_music();
            return;
        }
        let fade_seconds = fade_duration as f32 / 1000.0;
        self.music_gain_target = 0.0;
        self.music_gain_step = -1.0 / (fade_seconds * INTERNAL_RATE);
        self.stop_when_silent = true;
    }

    /// Advance the music-gain envelope one sample; stop the song if a fade-out
    /// has reached silence.
    fn advance_music_gain(&mut self) {
        if self.music_gain_step == 0.0 {
            return;
        }
        self.music_gain += self.music_gain_step;
        let reached = if self.music_gain_step > 0.0 {
            self.music_gain >= self.music_gain_target
        } else {
            self.music_gain <= self.music_gain_target
        };
        if reached {
            self.music_gain = self.music_gain_target;
            self.music_gain_step = 0.0;
            if self.stop_when_silent {
                self.stop_music();
            }
        }
    }

    pub fn stop_music(&mut self) {
        for v in &mut self.voices {
            if v.as_ref().is_some_and(|v| v.from_music) {
                *v = None;
            }
        }
        self.music_state = None;
        self.current_token = 0;
        self.music_gain = 1.0;
        self.music_gain_target = 1.0;
        self.music_gain_step = 0.0;
        self.stop_when_silent = false;
        self.reserved_channels = 0;
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
                    self.voices[ch] = Some(Voice::new(*sfx_idx as usize, sfx, true));
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

    /// Render one mono sample at the device rate.
    ///
    /// The synth core runs at `INTERNAL_RATE`; this resamples up to the
    /// device rate with linear interpolation, then applies a two-pole
    /// reconstruction low-pass to suppress interpolation imaging and match
    /// PICO-8's clean top end. Calling it N times advances device time by
    /// `N / sample_rate` seconds.
    pub fn next_sample(&mut self) -> f32 {
        // Internal samples consumed per output sample (< 1 when upsampling).
        let ratio = INTERNAL_RATE / self.sample_rate;
        self.resample_frac += ratio;
        while self.resample_frac >= 1.0 {
            self.prev_internal = self.cur_internal;
            self.cur_internal = self.render_internal();
            self.resample_frac -= 1.0;
        }
        let mut out =
            self.prev_internal + (self.cur_internal - self.prev_internal) * self.resample_frac;
        // Two-pole reconstruction low-pass at ~11 kHz on the device-rate
        // stream: lp1 filters `out`, then lp2 filters lp1.
        let fc = 11_000.0;
        let dt_dev = 1.0 / self.sample_rate;
        let alpha = dt_dev / (1.0 / (2.0 * std::f32::consts::PI * fc) + dt_dev);
        self.lp1 += alpha * (out - self.lp1);
        self.lp2 += alpha * (self.lp1 - self.lp2);
        out = self.lp2;
        out
    }

    /// Render one mono sample at the internal rate.
    fn render_internal(&mut self) -> f32 {
        let dt = 1.0 / INTERNAL_RATE;
        self.t += dt;

        if let Some(state) = &mut self.music_state {
            state.remaining -= dt;
            if state.remaining <= 0.0 {
                self.advance_music();
            }
        }

        // Timbre of the eight SFX slots usable as custom instruments: each
        // slot's note-0 built-in waveform and its drawn waveform table (if any).
        let mut inst_waves = [0u8; 8];
        let mut inst_drawn: [Option<crate::assets::CustomWave>; 8] = Default::default();
        for i in 0..8 {
            if let Some(s) = self.sfx.get(i) {
                inst_waves[i] = s.notes[0].wave_index();
                inst_drawn[i] = s.custom_wave;
            }
        }

        let mut music_mix = 0.0;
        let mut sfx_mix = 0.0;
        for v in &mut self.voices {
            if let Some(voice) = v {
                let from_music = voice.from_music;
                match voice.sample(dt, self.t, &inst_waves, &inst_drawn) {
                    Some(s) => {
                        if from_music {
                            music_mix += s;
                        } else {
                            sfx_mix += s;
                        }
                    }
                    None => *v = None,
                }
            }
        }
        self.advance_music_gain();
        (sfx_mix + music_mix * self.music_gain).clamp(-1.0, 1.0)
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

    pub fn play_music(&self, n: i32, fade_duration: i32, channel_mask: i32, token: i32) -> i32 {
        self.with_synth(|s| s.play_music(n, fade_duration, channel_mask, token))
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
                |err| eprintln!("RICO-8 audio error: {err}"),
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
        // Default speed 16 -> 32 steps * 16 * 183 / 22050 s ~= 4.25 s; play 5 s.
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
    fn noise_is_smooth_not_crackly() {
        // Mirror airwolf's percussion: every step a wave-6 (noise) note at a
        // fixed pitch, full speed, with buzz on. The old hard sample-and-hold
        // noise (resampled LFSR + tanh overdrive) slams steps into the rails,
        // crackling; PICO-8's leaky-integrator noise stays smooth.
        let mut sfx = vec![Sfx::default(); SFX_COUNT];
        for note in sfx[0].notes.iter_mut() {
            *note = Note {
                pitch: 17,
                wave: 6,
                volume: 7,
                effect: 0,
            };
        }
        sfx[0].speed = 16;
        sfx[0].buzz = true;
        sfx[0].noiz = false;

        let mut synth = Synth::new(48000.0);
        synth.load(sfx, vec![MusicPattern::default(); 64]);
        synth.play_sfx(0, 0);

        // Render ~0.5 s; skip the first 256 samples (anti-click onset ramp).
        let mut buf = Vec::with_capacity(24000);
        for _ in 0..24000 {
            buf.push(synth.next_sample());
        }
        let mut max_jump = 0.0f32;
        for i in 257..buf.len() {
            max_jump = max_jump.max((buf[i] - buf[i - 1]).abs());
        }
        let peak = buf[256..].iter().fold(0.0f32, |m, s| m.max(s.abs()));

        // Measured max sample-to-sample jump at the current gain (still the
        // `* 0.25` master): the leaky integrator is smooth at ~0.018, while the
        // old hard sample-and-hold would be ~0.146 here. 0.03 sits cleanly
        // between, so this distinguishes crackle from smooth.
        assert!(peak > 0.01, "noise should be audible: peak {peak}");
        assert!(
            max_jump < 0.03,
            "noise should be smooth, not crackly: max jump {max_jump}"
        );
    }

    #[test]
    fn note_transitions_do_not_click() {
        // A hard amplitude transition (volume 7 -> 0 between steps) on a
        // click-free triangle wave: the triangle has no in-waveform
        // discontinuity, so any large sample-to-sample jump can only come
        // from an un-ramped amplitude boundary (onset or note change).
        let mut sfx = vec![Sfx::default(); SFX_COUNT];
        sfx[0].speed = 16;
        sfx[0].notes[0] = Note {
            pitch: 33,
            wave: 0,
            volume: 7,
            effect: 0,
        };
        sfx[0].notes[1] = Note {
            pitch: 33,
            wave: 0,
            volume: 0,
            effect: 0,
        };
        let mut synth = Synth::new(48000.0);
        synth.load(sfx, vec![MusicPattern::default(); 64]);
        synth.play_sfx(0, 0);

        // Step length = 16 * 183 / 22050 ~= 0.133 s; render ~0.3 s so we
        // cross both the onset and the note0 -> note1 boundary.
        let mut buf = Vec::with_capacity(14400);
        for _ in 0..14400 {
            buf.push(synth.next_sample());
        }
        let mut max_jump = 0.0f32;
        for i in 1..buf.len() {
            max_jump = max_jump.max((buf[i] - buf[i - 1]).abs());
        }
        let peak = buf.iter().fold(0.0f32, |m, s| m.max(s.abs()));

        // Measured at this device rate and at the current gain (waveforms still
        // +-1.0, master still `* 0.25`): an un-ramped amplitude jump (onset and
        // the note0 -> note1 boundary, smeared by the 22050 -> 48000 resampler)
        // is ~0.067, while the 2.5 ms ramp leaves max_jump ~= 0.010, dominated
        // by the ramp's own per-sample step near peak rather than a
        // discontinuity. The 0.02 threshold sits cleanly between.
        assert!(
            max_jump < 0.02,
            "amplitude jump should be smooth: {max_jump}"
        );
        assert!(peak > 0.01, "the note should still be audible: {peak}");
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
        synth.play_music(0, 0, 0, 0);
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
        synth.play_music(1, 0, 0, 0);
        for _ in 0..(44100 * 5) {
            synth.next_sample();
        }
        assert_eq!(synth.playing_pattern(), Some(0), "should loop to start");
    }

    #[test]
    fn pattern_length_follows_first_non_looping_channel() {
        // ch0 is the timekeeper at speed 4 (32*4*183/22050 ~= 1.062s); ch1 is
        // four times longer. The pattern must end with ch0, not stretch to ch1.
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
        synth.play_music(0, 0, 0, 0);
        let mut n = 0;
        while synth.playing_pattern().is_some() && n < 44100 * 5 {
            synth.next_sample();
            n += 1;
        }
        let secs = n as f32 / 44100.0;
        assert!(
            (secs - 1.062).abs() < 0.03,
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
        synth.play_music(0, 0, 0, 0);
        synth.play_sfx(1, -1);
        let chans = synth.channel_sfx();
        assert_eq!(chans[0], Some(0), "music keeps channel 0");
        assert!(chans[1..].contains(&Some(1)), "sfx lands elsewhere");
    }

    #[test]
    fn drawn_waveform_instrument_drives_output() {
        use crate::assets::{CustomWave, Note, NOTE_CUSTOM_FLAG, SFX_COUNT, SFX_LEN};
        let mut sfx = vec![Sfx::default(); SFX_COUNT];
        // SFX 1 is a drawn-waveform instrument held at the maximum positive
        // sample: this produces a constant positive (DC) signal, which no
        // built-in (zero-mean) waveform could ever produce — so a nonzero
        // positive mean proves the drawn samples are what's being played.
        sfx[1].custom_wave = Some(CustomWave {
            samples: [15; SFX_LEN],
            bass: false,
        });
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
        let mut sum = 0.0f32;
        let n = 2000;
        for _ in 0..n {
            let s = synth.next_sample();
            assert!(s.is_finite() && s.abs() <= 1.0, "sample out of range: {s}");
            sum += s;
        }
        assert!(
            sum / n as f32 > 0.05,
            "drawn samples should drive the output"
        );
    }

    #[test]
    fn channel_step_tracks_playback() {
        let mut synth = Synth::new(44100.0);
        synth.load(test_sfx(), vec![MusicPattern::default(); 64]);
        assert_eq!(synth.channel_step(), [None, None, None, None]);
        synth.play_sfx(0, 0);
        // After starting, channel 0 is on step 0.
        assert_eq!(synth.channel_step()[0], Some(0));
        // Default speed 16 -> 16*183/22050 ~= 0.133 s/step; advance ~0.2 s,
        // expect step 1.
        for _ in 0..(44100 / 5) {
            synth.next_sample();
        }
        assert_eq!(synth.channel_step()[0], Some(1));
    }

    #[test]
    fn second_start_is_refused_while_playing() {
        let mut synth = Synth::new(44100.0);
        let mut music = vec![MusicPattern::default(); 64];
        music[0].channels[0] = Some(0);
        music[1].channels[0] = Some(0);
        synth.load(test_sfx(), music);
        let token = synth.play_music(0, 0, 0, 0);
        assert!(token != 0, "first start mints a nonzero token");
        // A second start while a song plays is refused.
        assert_eq!(synth.play_music(1, 0, 0, 0), 0);
        assert_eq!(synth.playing_pattern(), Some(0), "first song keeps playing");
    }

    #[test]
    fn stale_token_does_not_stop_a_later_song() {
        let mut synth = Synth::new(44100.0);
        let mut music = vec![MusicPattern::default(); 64];
        music[0].channels[0] = Some(0);
        music[0].stop_at_end = true; // one-shot: ends on its own
        music[1].channels[0] = Some(0);
        synth.load(test_sfx(), music);
        let stale = synth.play_music(0, 0, 0, 0);
        for _ in 0..(44100 * 5) {
            synth.next_sample(); // let song 0 finish
        }
        assert_eq!(synth.playing_pattern(), None, "one-shot ended on its own");
        let fresh = synth.play_music(1, 0, 0, 0);
        assert!(fresh != 0 && fresh != stale, "new song gets a fresh token");
        // A stop carrying the stale token must NOT stop the new song.
        synth.play_music(-1, 0, 0, stale);
        assert_eq!(synth.playing_pattern(), Some(1), "stale token is a no-op");
        // The fresh token stops it.
        synth.play_music(-1, 0, 0, fresh);
        assert_eq!(synth.playing_pattern(), None);
    }

    #[test]
    fn music_fades_in_from_silence() {
        let mut synth = Synth::new(44100.0);
        let mut music = vec![MusicPattern::default(); 64];
        // Loop the song so it never ends on its own during the measurement window.
        music[0].channels[0] = Some(0);
        music[0].loop_start = true;
        music[1].channels[0] = Some(0);
        music[1].loop_back = true;
        synth.load(test_sfx(), music);
        synth.play_music(0, 1000, 0, 0); // 1s fade-in
        assert!(
            synth.music_gain < 0.05,
            "starts near silent: {}",
            synth.music_gain
        );
        for _ in 0..(44100 / 2) {
            synth.next_sample();
        }
        assert!(
            synth.music_gain > 0.4 && synth.music_gain < 0.6,
            "~half after 0.5s: {}",
            synth.music_gain
        );
        for _ in 0..44100 {
            synth.next_sample();
        }
        assert!(
            (synth.music_gain - 1.0).abs() < 1e-3,
            "reaches full: {}",
            synth.music_gain
        );
    }

    #[test]
    fn music_fades_out_then_stops() {
        let mut synth = Synth::new(44100.0);
        let mut music = vec![MusicPattern::default(); 64];
        music[0].channels[0] = Some(0);
        music[0].loop_start = true; // loops, so it never ends on its own
        music[1].channels[0] = Some(0);
        music[1].loop_back = true;
        synth.load(test_sfx(), music);
        let token = synth.play_music(0, 0, 0, 0);
        synth.play_music(-1, 1000, 0, token); // 1s fade-out
        assert!(synth.stop_when_silent, "fading out");
        assert_eq!(
            synth.playing_pattern(),
            Some(0),
            "still playing while fading"
        );
        for _ in 0..(44100 / 2) {
            synth.next_sample();
        }
        assert!(synth.playing_pattern().is_some(), "still fading at 0.5s");
        for _ in 0..(44100 / 2 + 200) {
            synth.next_sample();
        }
        assert_eq!(synth.playing_pattern(), None, "stops once silent");
    }

    #[test]
    fn reserved_channel_is_not_auto_selected_for_sfx() {
        let mut synth = Synth::new(44100.0);
        let mut music = vec![MusicPattern::default(); 64];
        music[0].channels[0] = Some(0); // music plays on channel 0
        synth.load(test_sfx(), music);
        // Reserve channel 1, which is IDLE — so only the reservation (not mere
        // occupancy) can keep an auto-routed sfx off it. Without reservation the
        // router would pick idle channel 1 first.
        synth.play_music(0, 0, 0b0010, 0);
        synth.play_sfx(1, -1); // auto-routed
        let chans = synth.channel_sfx();
        assert_ne!(
            chans[1],
            Some(1),
            "sfx must avoid the reserved idle channel 1"
        );
        assert!(
            chans[2..].contains(&Some(1)),
            "sfx landed on a free channel"
        );
    }

    #[test]
    fn explicit_channel_overrides_reservation() {
        let mut synth = Synth::new(44100.0);
        let mut music = vec![MusicPattern::default(); 64];
        music[0].channels[0] = Some(0);
        synth.load(test_sfx(), music);
        synth.play_music(0, 0, 0b0001, 0); // reserve channel 0
        synth.play_sfx(1, 0); // explicit channel 0
        assert_eq!(synth.channel_sfx()[0], Some(1), "explicit request wins");
    }

    /// Goertzel single-bin DFT magnitude of `freq` (Hz) in `samples` at rate
    /// `fs`. Used to measure spectral content without a full FFT.
    fn goertzel(samples: &[f32], freq: f32, fs: f32) -> f32 {
        let omega = 2.0 * std::f32::consts::PI * freq / fs;
        let coeff = 2.0 * omega.cos();
        let mut s_prev = 0.0f32;
        let mut s_prev2 = 0.0f32;
        for &x in samples {
            let s = x + coeff * s_prev - s_prev2;
            s_prev2 = s_prev;
            s_prev = s;
        }
        let real = s_prev - s_prev2 * omega.cos();
        let imag = s_prev2 * omega.sin();
        (real * real + imag * imag).sqrt()
    }

    #[test]
    fn tick_duration_matches_pico8() {
        // PICO-8 times a speed-unit tick as 183 samples at 22050 Hz, not
        // 1/128 s. A 32-note, speed-16, non-looping SFX should last exactly
        // 32 * 16 * 183 / 22050 seconds. This fails on the old 1/128 timing.
        let mut sfx = Sfx {
            speed: 16,
            ..Default::default()
        };
        for n in sfx.notes.iter_mut() {
            *n = Note {
                pitch: 33,
                wave: 0,
                volume: 5,
                effect: 0,
            };
        }
        let expected = 32.0 * 16.0 * 183.0 / 22050.0;
        assert!((sfx_duration(&sfx) - expected).abs() < 1e-4);
    }

    #[test]
    fn no_aliasing_above_internal_nyquist() {
        // A sustained max-pitch (pitch 63) saw has its fundamental near
        // 2490 Hz; its harmonics 5-8 sit at ~12.4/14.9/17.4/19.9 kHz, well
        // above the 11025 Hz internal Nyquist. Rendered pointwise at 48 kHz
        // those harmonics ring loudly; synthesizing at 22050 Hz and
        // reconstruction-filtering on the way up must crush them.
        //
        // The high band probes those four harmonics. Threshold:
        // high-band/fundamental ratio < 0.30. Measured with this fixture:
        // the naive 48 kHz code gives ~0.70 (high=580, fund=835); after the
        // fix it drops to ~0.034 (high=25, fund=747). 0.30 sits cleanly
        // between the two — FAILS before / PASSES after (verified both ways).
        let mut sfx = Sfx {
            speed: 1,
            ..Default::default()
        };
        for n in sfx.notes.iter_mut() {
            *n = Note {
                pitch: 63,
                wave: 2,
                volume: 7,
                effect: 0,
            };
        }
        let mut all = vec![Sfx::default(); SFX_COUNT];
        all[0] = sfx;
        let fs = 48000.0;
        let mut synth = Synth::new(fs);
        synth.load(all, vec![MusicPattern::default(); 64]);
        synth.play_sfx(0, 0);
        let mut buf = Vec::with_capacity(24000);
        for i in 0..24000 {
            let s = synth.next_sample();
            if i >= 512 {
                buf.push(s);
            }
        }
        // Pitch-63 saw fundamental, and its harmonics 5-8 (above the internal
        // Nyquist) as the high-band probes.
        let fund = goertzel(&buf, 2490.0, fs);
        let high: f32 = [12445.0, 14934.0, 17423.0, 19912.0]
            .iter()
            .map(|&f| goertzel(&buf, f, fs))
            .sum();
        let ratio = high / fund;
        assert!(
            ratio < 0.30,
            "high-band/fundamental ratio {ratio} should be small (fund={fund}, high={high})"
        );
    }

    #[test]
    fn start_is_allowed_while_fading_out() {
        let mut synth = Synth::new(44100.0);
        let mut music = vec![MusicPattern::default(); 64];
        music[0].channels[0] = Some(0);
        music[0].loop_start = true;
        music[1].channels[0] = Some(0);
        music[1].loop_back = true;
        synth.load(test_sfx(), music);
        let a = synth.play_music(0, 0, 0, 0);
        synth.play_music(-1, 1000, 0, a); // fade A out
        let b = synth.play_music(0, 0, 0, 0); // start during the fade
        assert!(b != 0 && b != a, "took over during fade-out");
        assert!(
            !synth.stop_when_silent,
            "the new song plays at full, not fading"
        );
    }
}
