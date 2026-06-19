//! RICO-8 cart player for handhelds and desktop-from-a-TTY: a pure-Rust frontend over
//! `rico8-runtime` with a KMS/evdev/ALSA backend (no SDL). Point it at a cart or a directory.

mod app;
mod picker;
mod platform;

use anyhow::{anyhow, Result};
use app::App;
use platform::null::NullPlatform;
use rico8_runtime::audio::AudioHandle;
use std::path::PathBuf;

/// Kept alive for the app's lifetime; dropping it stops audio. `Some(stream)` for the windowed
/// (cpal) backend, `None`/`()` for the others.
#[cfg(feature = "window")]
type AudioKeepalive = Option<rico8_runtime::audio::AudioOutput>;
#[cfg(not(feature = "window"))]
type AudioKeepalive = ();

fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();

    if args.first().map(String::as_str) == Some("--probe") {
        println!("rico8-player ok arch={}", std::env::consts::ARCH);
        return Ok(());
    }

    let (smoke, args) = match args.split_first() {
        Some((flag, rest)) if flag == "--smoke" => {
            let (n, rest) = rest
                .split_first()
                .ok_or_else(|| anyhow!("--smoke <frames> <cart>"))?;
            (Some(n.parse::<u32>()?), rest.to_vec())
        }
        _ => (None, args),
    };
    let target = args
        .first()
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."));

    // The audio keepalive must outlive `app`; dropping it stops the cpal stream.
    let (platform, audio, _audio) = build_backend(smoke)?;
    let mut app = App::new(platform, audio, smoke);
    if target.is_file() {
        app.play(&target)?;
    } else {
        app.picker(&target)?;
    }
    Ok(())
}

/// Build the display/input backend plus the audio handle the running cart writes to. The third
/// element is a keepalive whose drop stops audio (`Some(cpal stream)` for windowed, `()` for KMS).
///
/// `window` is preferred when enabled (the desktop default); `kms` drives the handheld build,
/// where `window` is off. A build with neither backend errors at runtime.
fn build_backend(
    smoke: Option<u32>,
) -> Result<(Box<dyn platform::Platform>, AudioHandle, AudioKeepalive)> {
    if smoke.is_some() {
        return Ok((
            Box::new(NullPlatform::new()),
            AudioHandle::dummy(),
            Default::default(),
        ));
    }
    real_backend()
}

/// The non-smoke backend, selected at compile time by the enabled feature.
#[cfg(feature = "window")]
fn real_backend() -> Result<(Box<dyn platform::Platform>, AudioHandle, AudioKeepalive)> {
    let audio_out = rico8_runtime::audio::AudioOutput::start();
    let audio = audio_out
        .as_ref()
        .map(|a| a.handle())
        .unwrap_or_else(AudioHandle::dummy);
    let platform = platform::window::WindowPlatform::new()?;
    Ok((Box::new(platform), audio, audio_out))
}

#[cfg(all(feature = "kms", not(feature = "window")))]
fn real_backend() -> Result<(Box<dyn platform::Platform>, AudioHandle, AudioKeepalive)> {
    let audio = AudioHandle::dummy();
    let platform = platform::kms::KmsPlatform::new(audio.clone())?;
    Ok((Box::new(platform), audio, Default::default()))
}

#[cfg(not(any(feature = "window", feature = "kms")))]
fn real_backend() -> Result<(Box<dyn platform::Platform>, AudioHandle, AudioKeepalive)> {
    Err(anyhow!(
        "rico8-player was built with no display backend; enable the `window` or `kms` feature"
    ))
}
