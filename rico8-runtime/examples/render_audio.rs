//! Render a PICO-8 cart's SFX through RICO-8's synth to a WAV file, so the
//! output can be compared against PICO-8's own audio.
//!
//! Usage:
//!   cargo run --no-default-features --example render_audio -- <cart> <out.wav> [sfx_index]
//!
//! With an explicit `sfx_index` only that SFX is rendered (handy for tuning a
//! single filter); otherwise every non-empty SFX is played in sequence with a
//! short gap, and a manifest of `index@offset` is printed to stderr.

use rico8_runtime::{audio::Synth, pico8};
use std::path::Path;

const SR: f32 = 44100.0;
const MAX_SECS: f32 = 3.0;
const GAP_SECS: f32 = 0.2;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 3 {
        eprintln!("usage: render_audio <cart> <out.wav> [sfx_index]");
        std::process::exit(2);
    }
    let assets = pico8::parse_file(Path::new(&args[1])).expect("parse cart");
    let only: Option<usize> = args.get(3).and_then(|s| s.parse().ok());

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
        let max = (MAX_SECS * SR) as usize;
        for _ in 0..max {
            samples.push(synth.next_sample());
            if synth.channel_sfx()[0].is_none() {
                break;
            }
        }
        samples.resize(samples.len() + gap, 0.0);
    }

    write_wav(Path::new(&args[2]), &samples, SR as u32).expect("write wav");
    eprintln!(
        "wrote {} ({:.2}s) to {}",
        samples.len(),
        samples.len() as f32 / SR,
        args[2]
    );
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
