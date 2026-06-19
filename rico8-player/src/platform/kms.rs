//! KMS/DRM display backend: dumb-buffer double-buffering, page-flip, auto-rotation, VT graphics
//! mode, evdev input, and ALSA audio — everything a headless handheld needs.

use crate::platform::{
    alsa::{self, AudioThread},
    blit,
    evdev::Input,
    InputSnapshot, Platform, Rotate,
};
use anyhow::{anyhow, Context, Result};
use drm::{
    buffer::{Buffer as DrmBuffer, DrmFourcc},
    control::{
        connector, crtc, dumbbuffer::DumbBuffer, framebuffer, Device as ControlDevice, Event, Mode,
        ModeTypeFlags, PageFlipFlags,
    },
    Device as DrmDevice,
};
use nix::poll::{poll, PollFd, PollFlags, PollTimeout};
use rico8_runtime::{audio::AudioHandle, fb::Framebuffer};
use std::os::fd::{AsFd, AsRawFd, BorrowedFd, OwnedFd};

// KD_TEXT = 0, KD_GRAPHICS = 1 — from <linux/kd.h>.
const KDSETMODE: u64 = 0x4B3A;
nix::ioctl_write_int_bad!(kd_setmode, KDSETMODE);

/// Minimal DRM device wrapper: an owned fd that satisfies the drm crate's device traits.
struct Card(OwnedFd);

impl AsFd for Card {
    fn as_fd(&self) -> BorrowedFd<'_> {
        self.0.as_fd()
    }
}

impl DrmDevice for Card {}
impl ControlDevice for Card {}

/// KMS/DRM display + evdev input + ALSA audio backend. Implements `Platform`.
///
/// On construction: opens the DRM card, picks a connected connector and preferred mode, allocates
/// two XRGB8888 dumb buffers for double-buffering, sets the VT to graphics mode, and performs the
/// initial `set_crtc`. On drop: restores the VT to text mode.
pub struct KmsPlatform {
    card: Card,
    crtc: crtc::Handle,
    connector: connector::Handle,
    mode: Mode,
    dumb: [DumbBuffer; 2],
    fb: [framebuffer::Handle; 2],
    /// Index of the currently displayed (front) buffer.
    front: usize,
    width: usize,
    height: usize,
    rotate: Rotate,
    input: Input,
    /// Held open so `Drop` can restore KD_TEXT.
    vt: Option<std::fs::File>,
    _audio: Option<AudioThread>,
}

impl KmsPlatform {
    /// Open the DRM device, pick an output, allocate double-buffers, and start audio.
    ///
    /// The device path defaults to `/dev/dri/card0`; set `RICO8_DRM_CARD` to override.
    pub fn new(audio: AudioHandle) -> Result<KmsPlatform> {
        let card = open_card()?;
        let (con, mode, crtc) = pick_output(&card)?;

        let (w, h) = mode.size();
        let (width, height) = (w as usize, h as usize);

        let (db0, fb0) = make_buffer(&card, w, h)?;
        let (db1, fb1) = make_buffer(&card, w, h)?;

        let rotate = Rotate::from_env_or(detect_rotation(&card, con.handle()));

        let vt = set_vt_graphics();

        // Initial modeset: put the front buffer on screen.
        card.set_crtc(crtc, Some(fb0), (0, 0), &[con.handle()], Some(mode))?;

        Ok(KmsPlatform {
            card,
            crtc,
            connector: con.handle(),
            mode,
            dumb: [db0, db1],
            fb: [fb0, fb1],
            front: 0,
            width,
            height,
            rotate,
            input: Input::new(),
            vt,
            _audio: alsa::spawn(audio),
        })
    }
}

impl Platform for KmsPlatform {
    fn poll(&mut self) -> InputSnapshot {
        self.input.poll()
    }

    fn present(&mut self, fb: &Framebuffer) -> Result<()> {
        let back = 1 - self.front;
        blit_to_dumb(
            &self.card,
            &mut self.dumb[back],
            self.width,
            self.height,
            fb,
            self.rotate,
        )?;
        self.card
            .page_flip(self.crtc, self.fb[back], PageFlipFlags::EVENT, None)?;
        wait_for_page_flip(&self.card)?;
        self.front = back;
        Ok(())
    }
}

impl Drop for KmsPlatform {
    fn drop(&mut self) {
        if let Some(f) = &self.vt {
            // Obvious FFI: an ioctl taking only the fd and an int mode.
            let _ = unsafe { kd_setmode(f.as_raw_fd(), 0) }; // KD_TEXT.
        }
    }
}

// ── helpers ──────────────────────────────────────────────────────────────────

/// Open the DRM device at `RICO8_DRM_CARD` (default `/dev/dri/card0`).
fn open_card() -> Result<Card> {
    let path = std::env::var("RICO8_DRM_CARD").unwrap_or_else(|_| "/dev/dri/card0".into());
    let file = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(&path)
        .with_context(|| format!("opening {path}"))?;
    Ok(Card(OwnedFd::from(file)))
}

/// Find a connected connector, its preferred (or first) mode, and a compatible CRTC.
fn pick_output(card: &Card) -> Result<(connector::Info, Mode, crtc::Handle)> {
    let res = card.resource_handles()?;
    let con = res
        .connectors()
        .iter()
        .filter_map(|&h| card.get_connector(h, true).ok())
        .find(|c| c.state() == connector::State::Connected)
        .ok_or_else(|| anyhow!("no connected display"))?;
    let mode = con
        .modes()
        .iter()
        .find(|m| m.mode_type().contains(ModeTypeFlags::PREFERRED))
        .or_else(|| con.modes().first())
        .copied()
        .ok_or_else(|| anyhow!("connector has no modes"))?;
    let enc_handle = con
        .current_encoder()
        .or_else(|| con.encoders().first().copied())
        .ok_or_else(|| anyhow!("connector has no encoder"))?;
    let enc = card.get_encoder(enc_handle)?;
    let crtc = res
        .filter_crtcs(enc.possible_crtcs())
        .first()
        .copied()
        .ok_or_else(|| anyhow!("no usable CRTC"))?;
    Ok((con, mode, crtc))
}

/// Allocate one XRGB8888 dumb buffer and register it as a framebuffer.
fn make_buffer(card: &Card, w: u16, h: u16) -> Result<(DumbBuffer, framebuffer::Handle)> {
    let db = card.create_dumb_buffer((w.into(), h.into()), DrmFourcc::Xrgb8888, 32)?;
    let fb = card.add_framebuffer(&db, 24, 32)?;
    Ok((db, fb))
}

/// Blit `fb` into a dumb buffer, handling driver-padded pitch correctly.
///
/// Most drivers emit `pitch == width * 4` for XRGB8888, allowing a single
/// `bytemuck::cast_slice_mut` over the entire mapping. When the driver pads
/// rows (`pitch > width * 4`), each source row is written at the correct
/// byte offset so no pixels land in the padding area.
fn blit_to_dumb(
    card: &Card,
    dumb: &mut DumbBuffer,
    width: usize,
    height: usize,
    fb: &Framebuffer,
    rotate: Rotate,
) -> Result<()> {
    let pitch = dumb.pitch() as usize; // bytes per row, may be padded.
    let expected_pitch = width * 4; // bytes per row without padding.
    let mut map = card.map_dumb_buffer(dumb)?;
    if pitch == expected_pitch {
        // No padding: treat the whole mapping as a flat u32 slice.
        let dst: &mut [u32] = bytemuck::cast_slice_mut(map.as_mut());
        blit::present_into(fb, dst, width, height, rotate);
    } else {
        // Padded pitch: blit into a contiguous scratch buffer, then copy each
        // row at the correct byte offset into the mapping.
        let mut scratch = vec![0u32; width * height];
        blit::present_into(fb, &mut scratch, width, height, rotate);
        let raw: &mut [u8] = map.as_mut();
        for y in 0..height {
            let src_start = y * width;
            let dst_byte = y * pitch;
            let src_bytes = bytemuck::cast_slice::<u32, u8>(&scratch[src_start..src_start + width]);
            raw[dst_byte..dst_byte + expected_pitch].copy_from_slice(src_bytes);
        }
    }
    Ok(())
}

/// Block until the kernel delivers a page-flip completion event on `card`.
fn wait_for_page_flip(card: &Card) -> Result<()> {
    let mut pfds = [PollFd::new(card.as_fd(), PollFlags::POLLIN)];
    poll(&mut pfds, PollTimeout::NONE)?;
    for ev in card.receive_events()? {
        if let Event::PageFlip(_) = ev {
            break;
        }
    }
    Ok(())
}

/// Read the connector's `panel orientation` property and map it to a `Rotate`.
///
/// Returns `Rotate::None` when the property is absent or unreadable. The caller
/// passes this through `Rotate::from_env_or` so `RICO8_ROTATE` can override it.
fn detect_rotation(card: &Card, con: connector::Handle) -> Rotate {
    use drm::control::property::Value;
    let Ok(props) = card.get_properties(con) else {
        return Rotate::None;
    };
    for (&id, &raw) in props.iter() {
        let Ok(info) = card.get_property(id) else {
            continue;
        };
        if info.name().to_str() != Ok("panel orientation") {
            continue;
        }
        if let Value::Enum(Some(ev)) = info.value_type().convert_value(raw) {
            return match ev.name().to_str() {
                Ok("Upside Down") => Rotate::Cw180,
                // "Left Side Up" / "Right Side Up" map to CW90 / CW270 by convention;
                // confirm on-device — RICO8_ROTATE overrides if the panel is mirrored.
                Ok("Left Side Up") => Rotate::Cw90,
                Ok("Right Side Up") => Rotate::Cw270,
                _ => Rotate::None,
            };
        }
    }
    Rotate::None
}

/// Switch `/dev/tty0` to graphics mode (KD_GRAPHICS) so the VT console does not paint over us.
///
/// Returns the open file so `Drop` can restore KD_TEXT. Best-effort: returns `None` if the tty
/// cannot be opened (e.g. not running as root or in a VT).
fn set_vt_graphics() -> Option<std::fs::File> {
    let f = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open("/dev/tty0")
        .ok()?;
    // Obvious FFI: an ioctl taking only the fd and an int mode.
    let _ = unsafe { kd_setmode(f.as_raw_fd(), 1) }; // KD_GRAPHICS.
    Some(f)
}
