//! RICO-8 cart player for handhelds and desktop-from-a-TTY: a pure-Rust frontend over
//! `rico8-runtime` with a KMS/evdev/ALSA backend (no SDL). Point it at a cart or a directory.

mod app;
mod picker;
mod platform;

use anyhow::{anyhow, Result};
use app::App;
use platform::{kms::KmsPlatform, null::NullPlatform};
use std::path::PathBuf;

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

    let audio = rico8_runtime::audio::AudioHandle::dummy();
    let platform: Box<dyn platform::Platform> = if smoke.is_some() {
        Box::new(NullPlatform::new())
    } else {
        match KmsPlatform::new(audio.clone()) {
            Ok(p) => Box::new(p),
            Err(e) => {
                eprintln!("rico8-player: KMS init failed: {e:#}");
                return Err(e);
            }
        }
    };
    let mut app = App::new(platform, audio, smoke);
    if target.is_file() {
        app.play(&target)?;
    } else {
        app.picker(&target)?;
    }
    Ok(())
}
