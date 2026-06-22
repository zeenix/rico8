//! Render a PICO-8 cart's audio through RICO-8's synth to a WAV file, so the
//! output can be compared against PICO-8's own audio.
//!
//! Usage:
//!   cargo run --no-default-features --example render_audio -- <cart> <out.wav> [sfx_index|music]
//!
//! With `music`, the song (pattern 0 onward) is sequenced exactly as the
//! runtime plays it — all channels of a pattern start together — which is the
//! way to check that simultaneous tracks stay aligned. With an explicit
//! `sfx_index` only that SFX is rendered; otherwise every non-empty SFX plays
//! in sequence with a short gap. A manifest is printed to stderr.

use rico8_runtime::{assets::Assets, audio::Synth, pico8};
use std::path::Path;

const SR: f32 = 44100.0;
const MAX_SECS: f32 = 3.0;
const GAP_SECS: f32 = 0.2;
/// Cap for a (possibly looping) song render.
const MUSIC_SECS: f32 = 45.0;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 3 {
        eprintln!("usage: render_audio <cart> <out.wav> [sfx_index|music]");
        std::process::exit(2);
    }
    let assets = pico8::parse_file(Path::new(&args[1])).expect("parse cart");
    let arg = args.get(3).map(String::as_str);

    let samples = if arg == Some("music") {
        render_music(&assets)
    } else {
        render_sfx(&assets, arg.and_then(|s| s.parse().ok()))
    };

    write_wav(Path::new(&args[2]), &samples, SR as u32).expect("write wav");
    eprintln!(
        "wrote {} ({:.2}s) to {}",
        samples.len(),
        samples.len() as f32 / SR,
        args[2]
    );
}

/// Sequence the song the way the runtime does and capture it; logs each
/// pattern boundary so timing can be checked.
fn render_music(assets: &Assets) -> Vec<f32> {
    let mut synth = Synth::new(SR);
    synth.load(assets.sfx.clone(), assets.music.clone());
    synth.play_music(0, 0, 0, 0);
    let mut samples = Vec::new();
    let mut last = None;
    for _ in 0..(MUSIC_SECS * SR) as usize {
        let p = synth.playing_pattern();
        if p != last {
            let chans = synth.channel_sfx();
            let active: Vec<_> = chans.iter().filter_map(|c| *c).collect();
            eprintln!(
                "pattern {p:?} @ {:.2}s  channels={chans:?} active={active:?}",
                samples.len() as f32 / SR
            );
            last = p;
        }
        samples.push(synth.next_sample());
        if synth.playing_pattern().is_none() {
            break;
        }
    }
    samples
}

/// Render either one SFX (`only`) or every non-empty SFX back to back.
fn render_sfx(assets: &Assets, only: Option<usize>) -> Vec<f32> {
    let mut samples: Vec<f32> = Vec::new();
    let gap = (GAP_SECS * SR) as usize;
    for (idx, sfx) in assets.sfx.iter().enumerate() {
        if let Some(want) = only {
            if idx != want {
                continue;
            }
        } else if sfx.is_empty() {
            continue;
        }
        eprintln!(
            "sfx {idx:02} @ {:.2}s  speed={} noiz={} buzz={} detune={} reverb={} dampen={}",
            samples.len() as f32 / SR,
            sfx.speed,
            sfx.noiz,
            sfx.buzz,
            sfx.detune,
            sfx.reverb,
            sfx.dampen
        );
        let mut synth = Synth::new(SR);
        synth.load(assets.sfx.clone(), assets.music.clone());
        synth.play_sfx(idx as i32, 0);
        for _ in 0..(MAX_SECS * SR) as usize {
            samples.push(synth.next_sample());
            if synth.channel_sfx()[0].is_none() {
                break;
            }
        }
        samples.resize(samples.len() + gap, 0.0);
    }
    samples
}

/// Minimal mono 16-bit PCM WAV writer (no external deps).
fn write_wav(path: &Path, samples: &[f32], sr: u32) -> std::io::Result<()> {
    let mut buf = Vec::with_capacity(44 + samples.len() * 2);
    let data_len = (samples.len() * 2) as u32;
    let byte_rate = sr * 2;
    buf.extend_from_slice(b"RIFF");
    buf.extend_from_slice(&(36 + data_len).to_le_bytes());
    buf.extend_from_slice(b"WAVE");
    buf.extend_from_slice(b"fmt ");
    buf.extend_from_slice(&16u32.to_le_bytes());
    buf.extend_from_slice(&1u16.to_le_bytes()); // PCM
    buf.extend_from_slice(&1u16.to_le_bytes()); // mono
    buf.extend_from_slice(&sr.to_le_bytes());
    buf.extend_from_slice(&byte_rate.to_le_bytes());
    buf.extend_from_slice(&2u16.to_le_bytes()); // block align
    buf.extend_from_slice(&16u16.to_le_bytes()); // bits
    buf.extend_from_slice(b"data");
    buf.extend_from_slice(&data_len.to_le_bytes());
    for &s in samples {
        let v = (s.clamp(-1.0, 1.0) * 32767.0) as i16;
        buf.extend_from_slice(&v.to_le_bytes());
    }
    std::fs::write(path, buf)
}
