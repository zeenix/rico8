//! Raw-ioctl ALSA PCM playback (no libasound): open /dev/snd, set S16 stereo, write from the
//! synth on a thread. Best-effort: any failure means silent playback.

use nix::{ioctl_none, ioctl_readwrite, ioctl_write_ptr};
use rico8_runtime::audio::AudioHandle;
use std::os::fd::AsRawFd;

const SNDRV_PCM_VERSION: u32 = 0x0002_0012;
const ACCESS_RW_INTERLEAVED: u32 = 3;
const FORMAT_S16_LE: u32 = 2;
// Param indices.
const P_ACCESS: u32 = 0;
const P_FORMAT: u32 = 1;
const P_CHANNELS: u32 = 10;
const P_RATE: u32 = 11;
const P_PERIOD_SIZE: u32 = 13;
const P_PERIODS: u32 = 15;

const PERIOD: usize = 1024;
const PERIODS: u32 = 4;
// Raw hw devices commonly require stereo; the mono synth is duplicated.
const CHANNELS: usize = 2;

/// Nearest-neighbour resample of interleaved S16 frames from `from` Hz to `to` Hz.
pub fn resample_to(samples: &[i16], channels: usize, from: u32, to: u32) -> Vec<i16> {
    if from == to || channels == 0 {
        return samples.to_vec();
    }
    let in_frames = samples.len() / channels;
    let out_frames = (in_frames as u64 * to as u64 / from as u64) as usize;
    let mut out = Vec::with_capacity(out_frames * channels);
    for of in 0..out_frames {
        let src = (of as u64 * from as u64 / to as u64) as usize;
        let base = src.min(in_frames.saturating_sub(1)) * channels;
        for c in 0..channels {
            out.push(samples[base + c]);
        }
    }
    out
}

pub struct AudioThread {
    stop: std::sync::Arc<std::sync::atomic::AtomicBool>,
    handle: Option<std::thread::JoinHandle<()>>,
}

impl Drop for AudioThread {
    fn drop(&mut self) {
        self.stop.store(true, std::sync::atomic::Ordering::Relaxed);
        if let Some(h) = self.handle.take() {
            let _ = h.join();
        }
    }
}

/// Open the default PCM, negotiate S16 stereo (44100 or 48000), and start a writer thread.
/// Returns `None` (silent) on any failure or when `RICO8_NOAUDIO` is set.
pub fn spawn(audio: AudioHandle) -> Option<AudioThread> {
    if std::env::var_os("RICO8_NOAUDIO").is_some() {
        return None;
    }
    let file = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open("/dev/snd/pcmC0D0p")
        .ok()?;
    let fd = file.as_raw_fd();

    // Negotiate hw params, trying 44100 then 48000.
    let mut rate = 44100u32;
    let set_hw = |r: u32| -> nix::Result<()> {
        // SAFETY: SndPcmHwParams is repr(C) of integers and integer arrays, for which the
        // all-zero bit pattern is a valid value.
        let mut hw: SndPcmHwParams = unsafe { std::mem::zeroed() };
        param_init(&mut hw);
        set_mask(&mut hw, P_ACCESS, ACCESS_RW_INTERLEAVED);
        set_mask(&mut hw, P_FORMAT, FORMAT_S16_LE);
        set_int(&mut hw, P_CHANNELS, CHANNELS as u32);
        set_int(&mut hw, P_RATE, r);
        set_int(&mut hw, P_PERIOD_SIZE, PERIOD as u32);
        set_int(&mut hw, P_PERIODS, PERIODS);
        // SAFETY: `fd` is an open PCM device and `hw` is a correctly-laid-out
        // snd_pcm_hw_params (size asserted above), satisfying the ioctl's contract.
        unsafe { pcm_hw_params(fd, &mut hw) }.map(|_| ())
    };
    set_hw(44100)
        .or_else(|_| {
            rate = 48000;
            set_hw(48000)
        })
        .ok()?;

    // sw_params: auto-start once a full buffer is queued; block on full.
    let buffer = PERIOD * PERIODS as usize;
    let mut boundary = buffer;
    while boundary
        .checked_mul(2)
        .is_some_and(|b| b <= isize::MAX as usize)
    {
        boundary *= 2;
    }
    // SAFETY: SndPcmSwParams is repr(C) of integers; the all-zero bit pattern is valid.
    let mut sw: SndPcmSwParams = unsafe { std::mem::zeroed() };
    sw.period_step = 1;
    sw.avail_min = PERIOD;
    sw.start_threshold = buffer;
    sw.stop_threshold = boundary;
    sw.boundary = boundary;
    sw.proto = SNDRV_PCM_VERSION;
    // SAFETY: `fd` is the open PCM device; `sw` is a valid snd_pcm_sw_params.
    unsafe { pcm_sw_params(fd, &mut sw) }.ok()?;
    // Obvious FFI: an ioctl taking only the fd.
    unsafe { pcm_prepare(fd) }.ok()?;

    let stop = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let stop_t = stop.clone();
    let handle = std::thread::Builder::new()
        .name("rico8-audio".into())
        .spawn(move || writer_loop(file, rate, audio, stop_t))
        .ok()?;
    Some(AudioThread {
        stop,
        handle: Some(handle),
    })
}

/// Pull mono samples from the synth, duplicate to stereo, resample if needed, and write.
fn writer_loop(
    file: std::fs::File,
    rate: u32,
    audio: AudioHandle,
    stop: std::sync::Arc<std::sync::atomic::AtomicBool>,
) {
    use std::sync::atomic::Ordering;
    let fd = file.as_raw_fd();
    let mut mono = vec![0f32; PERIOD];
    while !stop.load(Ordering::Relaxed) {
        // Fill from the synth; contain a panic across the pull like the old SDL callback did.
        let pulled = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            audio.with_synth(|s| {
                for o in mono.iter_mut() {
                    *o = s.next_sample();
                }
            });
        }));
        if pulled.is_err() {
            mono.iter_mut().for_each(|o| *o = 0.0);
        }
        // Mono f32 -> interleaved stereo i16 at 44100.
        let mut stereo = Vec::with_capacity(PERIOD * CHANNELS);
        for &m in &mono {
            let s = (m.clamp(-1.0, 1.0) * i16::MAX as f32) as i16;
            stereo.push(s);
            stereo.push(s);
        }
        let frames_buf = if rate != 44100 {
            resample_to(&stereo, CHANNELS, 44100, rate)
        } else {
            stereo
        };
        // Write all frames, recovering from underruns.
        let total = frames_buf.len() / CHANNELS;
        let mut off = 0usize;
        while off < total {
            // SAFETY: off < total, so off * CHANNELS is in bounds of frames_buf.
            let ptr = unsafe { frames_buf.as_ptr().add(off * CHANNELS) };
            let x = SndXferi {
                result: 0,
                buf: ptr as *const _,
                frames: total - off,
            };
            // SAFETY: `fd` is the open PCM device; `x.buf` points to `x.frames` valid frames.
            match unsafe { pcm_writei(fd, &x as *const _) } {
                Ok(_) => {
                    let w = x.result.max(0) as usize;
                    if w == 0 {
                        break;
                    }
                    off += w;
                }
                Err(nix::errno::Errno::EPIPE) => {
                    // Obvious FFI: an ioctl taking only the fd.
                    let _ = unsafe { pcm_prepare(fd) };
                }
                Err(nix::errno::Errno::EINTR) => {}
                Err(_) => return,
            }
        }
    }
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
struct SndInterval {
    min: u32,
    max: u32,
    // openmin b0, openmax b1, integer b2, empty b3.
    flags: u32,
}

#[repr(C)]
#[derive(Clone, Copy)]
struct SndMask {
    bits: [u32; 8],
}

#[repr(C)]
struct SndPcmHwParams {
    flags: u32,
    masks: [SndMask; 3],
    mres: [SndMask; 5],
    intervals: [SndInterval; 12],
    ires: [SndInterval; 9],
    rmask: u32,
    cmask: u32,
    info: u32,
    msbits: u32,
    rate_num: u32,
    rate_den: u32,
    // snd_pcm_uframes_t.
    fifo_size: usize,
    sync: [u8; 16],
    reserved: [u8; 48],
}

#[repr(C)]
struct SndPcmSwParams {
    tstamp_mode: i32,
    period_step: u32,
    sleep_min: u32,
    avail_min: usize,
    xfer_align: usize,
    start_threshold: usize,
    stop_threshold: usize,
    silence_threshold: usize,
    silence_size: usize,
    boundary: usize,
    proto: u32,
    tstamp_type: u32,
    reserved: [u8; 56],
}

#[repr(C)]
struct SndXferi {
    // Frames written, set by kernel.
    result: isize,
    buf: *const std::ffi::c_void,
    frames: usize,
}

// Guard the layout (64-bit; armhf shrinks the uframes fields, so only assert on 64-bit).
#[cfg(target_pointer_width = "64")]
const _: () = assert!(std::mem::size_of::<SndPcmHwParams>() == 608);

ioctl_readwrite!(pcm_hw_params, b'A', 0x11, SndPcmHwParams);
ioctl_readwrite!(pcm_sw_params, b'A', 0x13, SndPcmSwParams);
ioctl_none!(pcm_prepare, b'A', 0x40);
ioctl_write_ptr!(pcm_writei, b'A', 0x50, SndXferi);

fn param_init(p: &mut SndPcmHwParams) {
    // p is zeroed by the caller.
    for m in &mut p.masks {
        m.bits[0] = !0;
        m.bits[1] = !0;
    }
    for iv in &mut p.intervals {
        iv.min = 0;
        iv.max = !0;
    }
    p.rmask = !0;
}

fn set_mask(p: &mut SndPcmHwParams, n: u32, bit: u32) {
    let m = &mut p.masks[n as usize];
    m.bits = [0; 8];
    m.bits[(bit >> 5) as usize] |= 1 << (bit & 31);
}

fn set_int(p: &mut SndPcmHwParams, n: u32, val: u32) {
    let iv = &mut p.intervals[(n - 8) as usize];
    iv.min = val;
    iv.max = val;
    // integer flag.
    iv.flags |= 1 << 2;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resample_44k_to_48k_lengthens() {
        // Stereo (2ch) input of 100 frames -> ~109 frames at 48k. Nearest-neighbour.
        let frames = 100;
        let input: Vec<i16> = (0..frames * 2).map(|i| i as i16).collect();
        let out = resample_to(&input, 2, 44100, 48000);
        let out_frames = out.len() / 2;
        assert!((108..=110).contains(&out_frames), "got {out_frames}");
        assert_eq!(out[0], input[0], "first frame preserved");
    }

    #[test]
    fn resample_same_rate_is_identity() {
        let input: Vec<i16> = vec![1, 2, 3, 4];
        assert_eq!(resample_to(&input, 2, 44100, 44100), input);
    }
}
