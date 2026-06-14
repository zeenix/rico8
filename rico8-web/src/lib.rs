//! The RICO-8 browser player.
//!
//! This crate is the desktop console's engine room — `rico8-runtime`,
//! wasmi sandbox and synthesizer included — compiled to WebAssembly and
//! wrapped in a tiny, explicit, C-like export surface. In the browser
//! the page's JavaScript is as thin as the desktop's wgpu layer: blit
//! the RGBA framebuffer to a canvas, map keys to buttons, feed audio
//! samples to WebAudio. The cart, the rasterizer, the font, the synth —
//! all bit-identical to the desktop console.
//!
//! Yes, that means a wasm interpreter (wasmi) running *inside* wasm.
//! Carts are 128x128 games at 30 or 60 fps; the double hop is well within
//! budget, and it keeps one implementation of everything.
//!
//! ## Export surface (all `extern "C"`, single-threaded)
//!
//! ```text
//! rico8_web_upload_begin(len) -> ptr   stage a cart.png upload
//! rico8_web_load() -> 0|1              parse + boot the staged cart
//! rico8_web_fps() -> 30|60             the cart's logical frame rate
//! rico8_web_error_ptr/len()            UTF-8 error text after a failure
//! rico8_web_set_button(b, down)        buttons 0..6, like the ABI
//! rico8_web_tick() -> 0|1              one logical frame; 1 = cart error
//! rico8_web_fb_ptr() -> ptr            128*128*4 RGBA, valid after tick
//! rico8_web_audio_render(n) -> n       render mono f32 samples @ 44100
//! rico8_web_audio_ptr() -> ptr         the rendered samples
//! ```

use rico8_runtime::{
    audio::AudioHandle,
    cart,
    fb::{Framebuffer, HEIGHT, WIDTH},
    palette::col,
    vm::{GameVm, DEFAULT_FPS},
};
use std::cell::UnsafeCell;

/// Sample rate the synth renders at; the page's AudioContext resamples.
pub const SAMPLE_RATE: u32 = 44100;
/// Maximum samples per `rico8_web_audio_render` call.
pub const AUDIO_CHUNK_MAX: usize = 4096;

const FB_BYTES: usize = (WIDTH * HEIGHT * 4) as usize;

pub struct Player {
    vm: Option<GameVm>,
    audio: AudioHandle,
    rgba: Vec<u8>,
    audio_buf: Vec<f32>,
    errored: bool,
}

impl Player {
    /// Boot a cart from PNG bytes.
    pub fn load(png: &[u8]) -> Result<Player, String> {
        let cart = cart::decode(png).map_err(|e| e.to_string())?;
        let audio = AudioHandle::dummy();
        let mut player = Player {
            vm: None,
            audio: audio.clone(),
            rgba: vec![0; FB_BYTES],
            audio_buf: vec![0.0; AUDIO_CHUNK_MAX],
            errored: false,
        };
        match GameVm::load(&cart.wasm, &cart.assets, audio) {
            Ok(vm) => {
                player.vm = Some(vm);
                Ok(player)
            }
            Err(e) => Err(e.to_string()),
        }
    }

    /// The cart's logical frame rate (30 or 60); 30 if no cart is loaded.
    pub fn fps(&self) -> u32 {
        self.vm.as_ref().map(GameVm::fps).unwrap_or(DEFAULT_FPS)
    }

    pub fn set_button(&mut self, b: usize, down: bool) {
        if let Some(vm) = &mut self.vm {
            vm.state_mut().input.set_button(b, down);
        }
    }

    /// Run one logical frame and refresh the RGBA buffer.
    /// Returns false once the cart has hit a runtime error; the buffer
    /// then holds the error screen.
    pub fn tick(&mut self) -> bool {
        if self.errored {
            return false;
        }
        let Some(vm) = &mut self.vm else {
            return false;
        };
        match vm.call_update().and_then(|()| vm.call_draw()) {
            Ok(()) => {
                vm.state().fb.write_rgba(&mut self.rgba);
                true
            }
            Err(e) => {
                self.fail(&e.to_string());
                false
            }
        }
    }

    fn fail(&mut self, message: &str) {
        self.errored = true;
        self.audio.stop_all();
        error_screen(message).write_rgba(&mut self.rgba);
    }

    pub fn rgba(&self) -> &[u8] {
        &self.rgba
    }

    /// Render up to `n` mono samples into the audio buffer; returns the
    /// number rendered.
    pub fn render_audio(&mut self, n: usize) -> usize {
        let n = n.min(AUDIO_CHUNK_MAX);
        let buf = &mut self.audio_buf[..n];
        self.audio.with_synth(|s| {
            for sample in buf.iter_mut() {
                *sample = s.next_sample();
            }
        });
        n
    }

    pub fn audio_buf(&self) -> &[f32] {
        &self.audio_buf
    }
}

/// The shared error screen plus a web-specific footer hint.
fn error_screen(message: &str) -> Framebuffer {
    let mut fb = rico8_runtime::ui::error_screen(message);
    fb.print("press f5 to restart", 2, HEIGHT - 7, col::LIGHT_GREY);
    fb
}

// ---------------------------------------------------------------------------
// C-like export surface over a single static player
// ---------------------------------------------------------------------------

struct Slot<T>(UnsafeCell<T>);

// The browser runs this module on one thread; there is no way to reach
// these statics concurrently.
unsafe impl<T> Sync for Slot<T> {}

static PLAYER: Slot<Option<Player>> = Slot(UnsafeCell::new(None));
static UPLOAD: Slot<Vec<u8>> = Slot(UnsafeCell::new(Vec::new()));
static ERROR: Slot<Vec<u8>> = Slot(UnsafeCell::new(Vec::new()));

#[allow(clippy::mut_from_ref)]
fn get<T>(slot: &Slot<T>) -> &mut T {
    unsafe { &mut *slot.0.get() }
}

/// Stage an upload buffer of `len` bytes; returns its address.
#[no_mangle]
pub extern "C" fn rico8_web_upload_begin(len: u32) -> *mut u8 {
    let upload = get(&UPLOAD);
    upload.clear();
    upload.resize(len as usize, 0);
    upload.as_mut_ptr()
}

/// Boot the staged cart. Returns 0 on success; on failure the error
/// text is available via `rico8_web_error_ptr/len`.
#[no_mangle]
pub extern "C" fn rico8_web_load() -> i32 {
    get(&ERROR).clear();
    match Player::load(get(&UPLOAD)) {
        Ok(player) => {
            *get(&PLAYER) = Some(player);
            0
        }
        Err(e) => {
            *get(&ERROR) = e.into_bytes();
            1
        }
    }
}

#[no_mangle]
pub extern "C" fn rico8_web_error_ptr() -> *const u8 {
    get(&ERROR).as_ptr()
}

#[no_mangle]
pub extern "C" fn rico8_web_error_len() -> u32 {
    get(&ERROR).len() as u32
}

/// The loaded cart's logical frame rate (30 or 60). The page reads this
/// after a successful `rico8_web_load` to set its fixed timestep.
#[no_mangle]
pub extern "C" fn rico8_web_fps() -> u32 {
    match get(&PLAYER) {
        Some(p) => p.fps(),
        None => DEFAULT_FPS,
    }
}

#[no_mangle]
pub extern "C" fn rico8_web_set_button(b: u32, down: i32) {
    if let Some(p) = get(&PLAYER) {
        p.set_button(b as usize, down != 0);
    }
}

/// One logical frame. Returns 0 while running, 1 once the cart errored
/// (the framebuffer then shows the error screen).
#[no_mangle]
pub extern "C" fn rico8_web_tick() -> i32 {
    match get(&PLAYER) {
        Some(p) => !p.tick() as i32,
        None => 1,
    }
}

#[no_mangle]
pub extern "C" fn rico8_web_fb_ptr() -> *const u8 {
    match get(&PLAYER) {
        Some(p) => p.rgba().as_ptr(),
        None => std::ptr::null(),
    }
}

#[no_mangle]
pub extern "C" fn rico8_web_audio_render(n: u32) -> u32 {
    match get(&PLAYER) {
        Some(p) => p.render_audio(n as usize) as u32,
        None => 0,
    }
}

#[no_mangle]
pub extern "C" fn rico8_web_audio_ptr() -> *const f32 {
    match get(&PLAYER) {
        Some(p) => p.audio_buf().as_ptr(),
        None => std::ptr::null(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rico8_runtime::{assets::Assets, cart::Cart};

    /// A real cart, end to end: WAT -> wasm -> PNG -> player.
    fn test_cart_png(wat_src: &str) -> Vec<u8> {
        let cart = Cart {
            wasm: wat::parse_str(wat_src).unwrap(),
            assets: Assets::default(),
            source: None,
        };
        cart::encode(&cart).unwrap()
    }

    const MOVER: &str = r#"
        (module
          (import "rico8" "cls" (func $cls (param i32)))
          (import "rico8" "pset" (func $pset (param i32 i32 i32)))
          (import "rico8" "btn" (func $btn (param i32) (result i32)))
          (global $x (mut i32) (i32.const 10))
          (func (export "rico8_init"))
          (func (export "rico8_update")
            (if (i32.ne (call $btn (i32.const 1)) (i32.const 0))
              (then (global.set $x (i32.add (global.get $x) (i32.const 1))))))
          (func (export "rico8_draw")
            (call $cls (i32.const 1))
            (call $pset (global.get $x) (i32.const 7) (i32.const 8)))
        )
    "#;

    #[test]
    fn player_runs_a_cart() {
        let png = test_cart_png(MOVER);
        let mut p = Player::load(&png).unwrap();
        assert!(p.tick());
        // Pixel (10,7) is color 8 = #ff004d.
        let i = (7 * 128 + 10) * 4;
        assert_eq!(&p.rgba()[i..i + 3], &[0xff, 0x00, 0x4d]);
        // Hold right for a frame; the pixel moves.
        p.set_button(1, true);
        assert!(p.tick());
        let i = (7 * 128 + 11) * 4;
        assert_eq!(&p.rgba()[i..i + 3], &[0xff, 0x00, 0x4d]);
    }

    #[test]
    fn cart_error_shows_error_screen() {
        let png = test_cart_png(
            r#"(module
                 (func (export "rico8_init"))
                 (func (export "rico8_update") (loop $l (br $l)))
                 (func (export "rico8_draw")))"#,
        );
        let mut p = Player::load(&png).unwrap();
        assert!(!p.tick(), "infinite loop must be trapped");
        assert!(!p.tick(), "stays errored");
        // The error screen's top bar is red.
        assert_eq!(&p.rgba()[0..3], &[0xff, 0x00, 0x4d]);
    }

    #[test]
    fn bad_cart_is_a_load_error() {
        assert!(Player::load(b"not a png").is_err());
    }

    #[test]
    fn exports_drive_the_player() {
        let png = test_cart_png(MOVER);
        let ptr = rico8_web_upload_begin(png.len() as u32);
        unsafe { std::ptr::copy_nonoverlapping(png.as_ptr(), ptr, png.len()) };
        assert_eq!(rico8_web_load(), 0);
        assert_eq!(rico8_web_tick(), 0);
        assert!(!rico8_web_fb_ptr().is_null());
        let n = rico8_web_audio_render(512);
        assert_eq!(n, 512);
    }

    #[test]
    fn load_failure_reports_error_text() {
        let ptr = rico8_web_upload_begin(4);
        unsafe { std::ptr::copy_nonoverlapping(b"oops".as_ptr(), ptr, 4) };
        assert_eq!(rico8_web_load(), 1);
        assert!(rico8_web_error_len() > 0);
    }
}
