//! WASM game execution: sandbox, host ABI, and lifecycle calls.
//!
//! Carts are `wasm32-unknown-unknown` modules executed with wasmi. The
//! only way a cart can touch the outside world is through the small,
//! C-like import set in the `"rico8"` module — no WASI, no filesystem,
//! no network. Fuel metering keeps runaway loops from hanging the
//! console; they surface as a friendly error screen instead.

use crate::{
    assets::{Assets, MapData, SpriteSheet},
    audio::AudioHandle,
    fb::Framebuffer,
    input::InputState,
};
use anyhow::{anyhow, Context as _, Result};
use wasmi::{
    Caller, Config, Engine, Instance, Linker, Module, Store, StoreLimits, StoreLimitsBuilder,
    TypedFunc,
};

/// A cart's logical frames per second when it doesn't say otherwise.
pub const DEFAULT_FPS: u32 = 60;

/// The console's own tick rate: editors, menus and cart pickers. Independent
/// of the cart rate, which the cart chooses via `rico8_fps`.
pub const UI_FPS: u32 = 30;

/// Fuel budget for a single lifecycle call. wasmi charges ~1 fuel per
/// instruction, so this is a hard cap of 131,072 (128 K) wasm instructions
/// per call — one number shared with the memory and cart-size limits. A real
/// frame uses a few thousand; exceeding this means the cart is stuck or doing
/// far too much, and surfaces as a friendly error screen.
const FUEL_PER_CALL: u64 = 131_072;

/// Hard cap on a cart's total linear memory: 128 K, the same number as the
/// fuel and cart-size limits. Covers static data, the shadow stack and the
/// heap together (wasm cannot separate them). Carts build with a 32 KiB stack
/// reserve, leaving up to ~96 KiB for static data and heap above it.
const MAX_MEMORY: usize = 131_072;

/// Everything the host exposes to a running cart.
pub struct HostState {
    pub fb: Framebuffer,
    pub input: InputState,
    pub sprites: SpriteSheet,
    pub map: MapData,
    pub audio: AudioHandle,
    /// Messages from the cart's `log` calls, drained by the console.
    pub logs: Vec<String>,
    /// Message from the cart's panic hook, captured just before the trap.
    pub panic_message: Option<String>,
    pub frame: u64,
    /// The cart's logical frames per second (30 or 60), from its `rico8_fps`
    /// export. Drives `time()` and the host's update/draw cadence.
    pub fps: u32,
    rng: u64,
    /// Enforces `MAX_MEMORY` on linear-memory growth, including the initial
    /// allocation at instantiation.
    limits: StoreLimits,
}

impl HostState {
    fn new(assets: &Assets, audio: AudioHandle) -> Self {
        Self {
            fb: Framebuffer::new(),
            input: InputState::default(),
            sprites: assets.sprites.clone(),
            map: assets.map.clone(),
            audio,
            logs: Vec::new(),
            panic_message: None,
            frame: 0,
            fps: DEFAULT_FPS,
            rng: 0x2545_f491_4f6c_dd1d,
            limits: StoreLimitsBuilder::new()
                .memory_size(MAX_MEMORY)
                .trap_on_grow_failure(true)
                .build(),
        }
    }

    fn next_rand(&mut self) -> f32 {
        // xorshift64*; carts that need determinism can bring their own RNG.
        let mut x = self.rng;
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        self.rng = x;
        let bits = (x.wrapping_mul(0x2545_f491_4f6c_dd1d) >> 40) as u32;
        bits as f32 / (1u32 << 24) as f32
    }

    fn seed_rand(&mut self, seed: u32) {
        // Force a nonzero xorshift state; all-zero is a fixed point.
        self.rng = (((seed as u64) << 32) | (seed as u64)) | 1;
    }
}

/// A cart-side runtime error, formatted for the error screen.
#[derive(Debug, Clone)]
pub struct RuntimeError {
    /// Which lifecycle call failed: "init", "update" or "draw".
    pub phase: &'static str,
    pub message: String,
}

impl std::fmt::Display for RuntimeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Runtime error in {}:\n{}", self.phase, self.message)
    }
}

/// A loaded, running cart.
pub struct GameVm {
    store: Store<HostState>,
    _instance: Instance,
    update: TypedFunc<(), ()>,
    draw: TypedFunc<(), ()>,
}

fn read_guest_str(caller: &Caller<'_, HostState>, ptr: u32, len: u32) -> String {
    let Some(mem) = caller
        .get_export("memory")
        .and_then(wasmi::Extern::into_memory)
    else {
        return String::new();
    };
    let data = mem.data(caller);
    let start = ptr as usize;
    let end = start.saturating_add(len as usize).min(data.len());
    if start >= end {
        return String::new();
    }
    String::from_utf8_lossy(&data[start..end]).into_owned()
}

/// Floor an `f32` screen coordinate to a pixel. The ABI carries positions
/// and sizes as floats so carts can hold sub-pixel state; the framebuffer is
/// an integer grid, so the host floors here, at the last moment, matching
/// PICO-8. Flooring (not truncation) keeps motion even across `x = 0`.
fn px(v: f32) -> i32 {
    v.floor() as i32
}

macro_rules! link {
    ($linker:expr, $name:literal, $f:expr) => {
        $linker
            .func_wrap("rico8", $name, $f)
            .with_context(|| format!("registering host fn {}", $name))?;
    };
}

impl GameVm {
    /// Load a cart module, wire up the ABI, and run `rico8_init`.
    pub fn load(wasm: &[u8], assets: &Assets, audio: AudioHandle) -> Result<Self> {
        // The single chokepoint every frontend runs a cart through: the
        // desktop console, the SDL player, the web player and headless
        // verify all land here. Reject mis-sized asset bundles before they
        // reach the renderer, regardless of where the cart came from (a PNG
        // cart, an on-disk project, or a hand-built module).
        crate::assets::validate(assets)?;

        let mut config = Config::default();
        config.consume_fuel(true);
        let engine = Engine::new(&config);
        let module = Module::new(&engine, wasm).map_err(|e| anyhow!("Invalid cart wasm: {e}"))?;

        audio.load(assets.sfx.clone(), assets.music.clone());
        let mut store = Store::new(&engine, HostState::new(assets, audio));
        store.limiter(|state| &mut state.limits);
        let mut linker = <Linker<HostState>>::new(&engine);

        link!(linker, "clear", |mut c: Caller<'_, HostState>, col: i32| {
            c.data_mut().fb.cls(col as u8)
        });
        link!(linker, "camera", |mut c: Caller<'_, HostState>,
                                 x: f32,
                                 y: f32| {
            c.data_mut().fb.camera(x, y)
        });
        link!(linker, "clip", |mut c: Caller<'_, HostState>,
                               x: f32,
                               y: f32,
                               w: f32,
                               h: f32| {
            c.data_mut().fb.clip(x, y, w, h)
        });
        link!(
            linker,
            "set_pixel",
            |mut c: Caller<'_, HostState>, x: f32, y: f32, col: i32| {
                c.data_mut().fb.pset(px(x), px(y), col as u8)
            }
        );
        link!(linker, "pixel", |c: Caller<'_, HostState>,
                                x: f32,
                                y: f32|
         -> i32 {
            c.data().fb.pget(px(x), px(y)) as i32
        });
        link!(linker, "line", |mut c: Caller<'_, HostState>,
                               x0: f32,
                               y0: f32,
                               x1: f32,
                               y1: f32,
                               col: i32| {
            c.data_mut()
                .fb
                .line(px(x0), px(y0), px(x1), px(y1), col as u8)
        });
        link!(linker, "rect", |mut c: Caller<'_, HostState>,
                               x0: f32,
                               y0: f32,
                               x1: f32,
                               y1: f32,
                               col: i32| {
            c.data_mut()
                .fb
                .rect(px(x0), px(y0), px(x1), px(y1), col as u8)
        });
        link!(
            linker,
            "rect_fill",
            |mut c: Caller<'_, HostState>, x0: f32, y0: f32, x1: f32, y1: f32, col: i32| {
                c.data_mut()
                    .fb
                    .rectfill(px(x0), px(y0), px(x1), px(y1), col as u8)
            }
        );
        link!(
            linker,
            "circle",
            |mut c: Caller<'_, HostState>, x: f32, y: f32, r: f32, col: i32| {
                c.data_mut().fb.circ(px(x), px(y), px(r), col as u8)
            }
        );
        link!(
            linker,
            "circle_fill",
            |mut c: Caller<'_, HostState>, x: f32, y: f32, r: f32, col: i32| {
                c.data_mut().fb.circfill(px(x), px(y), px(r), col as u8)
            }
        );
        link!(linker, "print", |mut c: Caller<'_, HostState>,
                                ptr: u32,
                                len: u32,
                                x: f32,
                                y: f32,
                                col: i32|
         -> f32 {
            let s = read_guest_str(&c, ptr, len);
            c.data_mut().fb.print(&s, px(x), px(y), col as u8) as f32
        });
        link!(linker, "is_button_down", |c: Caller<'_, HostState>,
                                         b: u32|
         -> i32 {
            c.data().input.btn(b) as i32
        });
        link!(linker, "is_button_pressed", |c: Caller<'_, HostState>,
                                            b: u32|
         -> i32 {
            c.data().input.btnp(b) as i32
        });
        link!(linker, "buttons_down", |c: Caller<'_, HostState>| -> i32 {
            c.data().input.btn_mask() as i32
        });
        link!(
            linker,
            "buttons_pressed",
            |c: Caller<'_, HostState>| -> i32 { c.data().input.btnp_mask() as i32 }
        );
        link!(
            linker,
            "sprite",
            |mut c: Caller<'_, HostState>,
             n: u32,
             x: f32,
             y: f32,
             w: f32,
             h: f32,
             flip_x: i32,
             flip_y: i32| {
                let HostState { fb, sprites, .. } = c.data_mut();
                fb.spr(sprites, n, x, y, w, h, flip_x != 0, flip_y != 0);
            }
        );
        link!(linker, "map", |mut c: Caller<'_, HostState>,
                              cel_x: i32,
                              cel_y: i32,
                              sx: f32,
                              sy: f32,
                              cel_w: i32,
                              cel_h: i32,
                              layers: u32| {
            let HostState {
                fb, sprites, map, ..
            } = c.data_mut();
            fb.map(
                map,
                sprites,
                cel_x,
                cel_y,
                sx,
                sy,
                cel_w,
                cel_h,
                layers as u8,
            );
        });
        link!(linker, "map_tile", |c: Caller<'_, HostState>,
                                   x: i32,
                                   y: i32|
         -> i32 {
            c.data().map.get(x, y) as i32
        });
        link!(
            linker,
            "set_map_tile",
            |mut c: Caller<'_, HostState>, x: i32, y: i32, v: u32| {
                c.data_mut().map.set(x, y, v as u8)
            }
        );
        link!(linker, "sprite_flags", |c: Caller<'_, HostState>,
                                       n: u32|
         -> i32 {
            c.data().sprites.flags(n) as i32
        });
        link!(
            linker,
            "set_sprite_flags",
            |mut c: Caller<'_, HostState>, n: u32, flags: u32| {
                c.data_mut().sprites.flags[(n as usize) % crate::assets::SPRITE_COUNT] =
                    flags as u8;
            }
        );
        link!(linker, "sfx", |c: Caller<'_, HostState>,
                              n: i32,
                              channel: i32| {
            c.data().audio.play_sfx(n, channel)
        });
        link!(linker, "music", |c: Caller<'_, HostState>, n: i32| {
            c.data().audio.play_music(n)
        });
        link!(linker, "time", |c: Caller<'_, HostState>| -> f32 {
            let st = c.data();
            st.frame as f32 / st.fps as f32
        });
        link!(linker, "rnd", |mut c: Caller<'_, HostState>| -> f32 {
            c.data_mut().next_rand()
        });
        link!(linker, "log", |mut c: Caller<'_, HostState>,
                              ptr: u32,
                              len: u32| {
            let s = read_guest_str(&c, ptr, len);
            c.data_mut().logs.push(s);
        });
        link!(linker, "panic", |mut c: Caller<'_, HostState>,
                                ptr: u32,
                                len: u32| {
            let s = read_guest_str(&c, ptr, len);
            c.data_mut().panic_message = Some(s);
        });
        link!(
            linker,
            "seed_rng",
            |mut c: Caller<'_, HostState>, seed: u32| { c.data_mut().seed_rand(seed) }
        );
        link!(linker, "sprite_pixel", |c: Caller<'_, HostState>,
                                       x: i32,
                                       y: i32|
         -> i32 {
            c.data().sprites.get(x, y) as i32
        });
        link!(
            linker,
            "set_sprite_pixel",
            |mut c: Caller<'_, HostState>, x: i32, y: i32, col: i32| {
                c.data_mut().sprites.set(x, y, col as u8)
            }
        );
        link!(
            linker,
            "sprite_stretch",
            |mut c: Caller<'_, HostState>,
             sx: i32,
             sy: i32,
             sw: i32,
             sh: i32,
             dx: f32,
             dy: f32,
             dw: f32,
             dh: f32,
             flip_x: i32,
             flip_y: i32| {
                let HostState { fb, sprites, .. } = c.data_mut();
                fb.sspr(
                    sprites,
                    sx,
                    sy,
                    sw,
                    sh,
                    dx,
                    dy,
                    px(dw),
                    px(dh),
                    flip_x != 0,
                    flip_y != 0,
                );
            }
        );
        link!(
            linker,
            "ellipse",
            |mut c: Caller<'_, HostState>, x0: f32, y0: f32, x1: f32, y1: f32, col: i32| {
                c.data_mut()
                    .fb
                    .oval(px(x0), px(y0), px(x1), px(y1), col as u8)
            }
        );
        link!(
            linker,
            "ellipse_fill",
            |mut c: Caller<'_, HostState>, x0: f32, y0: f32, x1: f32, y1: f32, col: i32| {
                c.data_mut()
                    .fb
                    .ovalfill(px(x0), px(y0), px(x1), px(y1), col as u8)
            }
        );
        link!(
            linker,
            "set_transparent_color",
            |mut c: Caller<'_, HostState>, col: i32, t: i32| {
                c.data_mut().fb.set_transparent_color(col as u8, t != 0)
            }
        );
        link!(linker, "reset_transparency", |mut c: Caller<
            '_,
            HostState,
        >| {
            c.data_mut().fb.reset_transparency()
        });
        link!(
            linker,
            "remap_color",
            |mut c: Caller<'_, HostState>, from: i32, to: i32, mode: i32| {
                let fb = &mut c.data_mut().fb;
                if mode == 0 {
                    fb.remap_color(from as u8, to as u8);
                } else {
                    fb.remap_display_color(from as u8, to as u8);
                }
            }
        );
        link!(linker, "reset_palette", |mut c: Caller<'_, HostState>| {
            c.data_mut().fb.reset_palette()
        });
        link!(
            linker,
            "set_fill_pattern",
            |mut c: Caller<'_, HostState>, pattern: i32, secondary: i32, transparent: i32| {
                c.data_mut()
                    .fb
                    .set_fill_pattern(pattern as u16, secondary as u8, transparent != 0)
            }
        );
        link!(
            linker,
            "set_pen_color",
            |mut c: Caller<'_, HostState>, col: i32| { c.data_mut().fb.set_pen_color(col as u8) }
        );
        link!(
            linker,
            "set_cursor",
            |mut c: Caller<'_, HostState>, x: f32, y: f32| {
                c.data_mut().fb.set_cursor(px(x), px(y))
            }
        );
        link!(linker, "print_pen", |mut c: Caller<'_, HostState>,
                                    ptr: u32,
                                    len: u32|
         -> f32 {
            let s = read_guest_str(&c, ptr, len);
            c.data_mut().fb.print_pen(&s) as f32
        });

        store
            .set_fuel(FUEL_PER_CALL)
            .map_err(|e| anyhow!("Fuel setup: {e}"))?;
        let instance = linker
            .instantiate_and_start(&mut store, &module)
            .map_err(|e| {
                let s = e.to_string();
                if s.contains("resource limiter denied") {
                    anyhow!("Cart needs more than 128K of memory to start")
                } else {
                    anyhow!("Cart does not match the RICO-8 ABI: {e}")
                }
            })?;

        let init = instance
            .get_typed_func::<(), ()>(&store, "rico8_init")
            .map_err(|e| anyhow!("Cart is missing rico8_init: {e}"))?;
        let update = instance
            .get_typed_func::<(), ()>(&store, "rico8_update")
            .map_err(|e| anyhow!("Cart is missing rico8_update: {e}"))?;
        let draw = instance
            .get_typed_func::<(), ()>(&store, "rico8_draw")
            .map_err(|e| anyhow!("Cart is missing rico8_draw: {e}"))?;

        let mut vm = Self {
            store,
            _instance: instance,
            update,
            draw,
        };
        vm.call("init", init).map_err(|e| anyhow!(e.to_string()))?;
        vm.store.data_mut().fps = vm.query_fps();
        Ok(vm)
    }

    /// Read the cart's `rico8_fps` export. The SDK emits it from every cart;
    /// 30 and 60 are honored, and anything else (or a hand-written cart with
    /// no such export) falls back to the default.
    fn query_fps(&mut self) -> u32 {
        let Ok(func) = self
            ._instance
            .get_typed_func::<(), u32>(&self.store, "rico8_fps")
        else {
            return DEFAULT_FPS;
        };
        self.store.set_fuel(FUEL_PER_CALL).ok();
        match func.call(&mut self.store, ()) {
            Ok(30) => 30,
            Ok(60) => 60,
            _ => DEFAULT_FPS,
        }
    }

    fn call(
        &mut self,
        phase: &'static str,
        func: TypedFunc<(), ()>,
    ) -> std::result::Result<(), RuntimeError> {
        self.store.set_fuel(FUEL_PER_CALL).ok();
        func.call(&mut self.store, ()).map_err(|err| {
            let message = match self.store.data_mut().panic_message.take() {
                Some(panic) => panic,
                None => {
                    let s = err.to_string();
                    if s.contains("fuel") {
                        format!("{phase}() ran too long\n(infinite loop?)")
                    } else if s.contains("growth operation limited") {
                        format!("{phase}() ran out of memory\n(128K limit)")
                    } else {
                        s
                    }
                }
            };
            RuntimeError { phase, message }
        })
    }

    /// Run one logical frame: tick input, call `rico8_update`.
    pub fn call_update(&mut self) -> std::result::Result<(), RuntimeError> {
        self.store.data_mut().input.tick();
        let r = self.call("update", self.update);
        self.store.data_mut().frame += 1;
        r
    }

    /// Call `rico8_draw`.
    pub fn call_draw(&mut self) -> std::result::Result<(), RuntimeError> {
        self.call("draw", self.draw)
    }

    /// The cart's logical frame rate: 30, or 60 if it opted in.
    pub fn fps(&self) -> u32 {
        self.store.data().fps
    }

    pub fn state(&self) -> &HostState {
        self.store.data()
    }

    pub fn state_mut(&mut self) -> &mut HostState {
        self.store.data_mut()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A minimal hand-written cart exercising the ABI from WAT.
    const TEST_CART: &str = r#"
        (module
          (import "rico8" "clear" (func $cls (param i32)))
          (import "rico8" "set_pixel" (func $pset (param f32 f32 i32)))
          (import "rico8" "pixel" (func $pget (param f32 f32) (result i32)))
          (import "rico8" "is_button_down" (func $btn (param i32) (result i32)))
          (import "rico8" "print" (func $print (param i32 i32 f32 f32 i32) (result f32)))
          (import "rico8" "log" (func $log (param i32 i32)))
          (memory (export "memory") 1)
          (data (i32.const 16) "hi from cart")
          (global $x (mut i32) (i32.const 5))
          (func (export "rico8_init")
            (call $log (i32.const 16) (i32.const 12)))
          (func (export "rico8_update")
            (if (i32.ne (call $btn (i32.const 1)) (i32.const 0))
              (then (global.set $x (i32.add (global.get $x) (i32.const 1))))))
          (func (export "rico8_draw")
            (call $cls (i32.const 1))
            (call $pset (f32.convert_i32_s (global.get $x)) (f32.const 7) (i32.const 8))
            (drop (call $print (i32.const 16) (i32.const 2) (f32.const 0) (f32.const 0) (i32.const 7))))
        )
    "#;

    const LOOPING_CART: &str = r#"
        (module
          (func (export "rico8_init"))
          (func (export "rico8_update") (loop $l (br $l)))
          (func (export "rico8_draw"))
        )
    "#;

    const FPS30_CART: &str = r#"
        (module
          (func (export "rico8_init"))
          (func (export "rico8_fps") (result i32) (i32.const 30))
          (func (export "rico8_update"))
          (func (export "rico8_draw")))
    "#;

    /// Update loops ~10k times — well under the 131,072-fuel budget.
    const BUDGET_OK_CART: &str = r#"
        (module
          (func (export "rico8_init"))
          (func (export "rico8_update")
            (local $i i32)
            (local.set $i (i32.const 10000))
            (loop $l
              (local.set $i (i32.add (local.get $i) (i32.const -1)))
              (br_if $l (local.get $i))))
          (func (export "rico8_draw")))
    "#;

    /// Update loops ~100k times — comfortably over the 131,072-fuel budget.
    const BUDGET_OVER_CART: &str = r#"
        (module
          (func (export "rico8_init"))
          (func (export "rico8_update")
            (local $i i32)
            (local.set $i (i32.const 100000))
            (loop $l
              (local.set $i (i32.add (local.get $i) (i32.const -1)))
              (br_if $l (local.get $i))))
          (func (export "rico8_draw")))
    "#;

    /// 1-page initial + grow by 1 page = 2 pages = exactly the 128 K cap (allowed).
    const GROW_TO_CAP_CART: &str = r#"
        (module
          (memory (export "memory") 1)
          (func (export "rico8_init"))
          (func (export "rico8_update") (drop (memory.grow (i32.const 1))))
          (func (export "rico8_draw")))
    "#;

    /// Update grows linear memory far past the 128 K cap (denied -> trap).
    const GROW_PAST_CAP_CART: &str = r#"
        (module
          (memory (export "memory") 1)
          (func (export "rico8_init"))
          (func (export "rico8_update") (drop (memory.grow (i32.const 10))))
          (func (export "rico8_draw")))
    "#;

    /// Declares 3 pages (192 KiB) of initial memory — over the 128 K cap, so it
    /// is denied at instantiation before the cart ever runs.
    const HUGE_INITIAL_MEMORY_CART: &str = r#"
        (module
          (memory (export "memory") 3)
          (func (export "rico8_init"))
          (func (export "rico8_update"))
          (func (export "rico8_draw")))
    "#;

    const PARITY_CART: &str = r#"
        (module
          (import "rico8" "ellipse" (func $ovalo (param f32 f32 f32 f32 i32)))
          (import "rico8" "ellipse_fill" (func $oval (param f32 f32 f32 f32 i32)))
          (import "rico8" "set_transparent_color" (func $palt (param i32 i32)))
          (import "rico8" "reset_transparency" (func $paltr))
          (import "rico8" "remap_color" (func $pal (param i32 i32 i32)))
          (import "rico8" "reset_palette" (func $palr))
          (import "rico8" "set_fill_pattern" (func $fillp (param i32 i32 i32)))
          (import "rico8" "set_sprite_pixel" (func $sset (param i32 i32 i32)))
          (import "rico8" "sprite_pixel" (func $sget (param i32 i32) (result i32)))
          (import "rico8" "sprite_stretch"
            (func $sspr (param i32 i32 i32 i32 f32 f32 f32 f32 i32 i32)))
          (import "rico8" "seed_rng" (func $srand (param i32)))
          (import "rico8" "set_pen_color" (func $color (param i32)))
          (import "rico8" "set_cursor" (func $cursor (param f32 f32)))
          (import "rico8" "print_pen" (func $printp (param i32 i32) (result f32)))
          (memory (export "memory") 1)
          (data (i32.const 0) "hi")
          (func (export "rico8_init"))
          (func (export "rico8_update")
            (call $srand (i32.const 42))
            (call $sset (i32.const 0) (i32.const 0) (i32.const 9))
            (drop (call $sget (i32.const 0) (i32.const 0))))
          (func (export "rico8_draw")
            (call $pal (i32.const 8) (i32.const 12) (i32.const 0))
            (call $palt (i32.const 0) (i32.const 1))
            (call $paltr)
            (call $fillp (i32.const 0) (i32.const 0) (i32.const 0))
            (call $color (i32.const 7))
            (call $cursor (f32.const 0) (f32.const 0))
            (drop (call $printp (i32.const 0) (i32.const 2)))
            (call $sspr (i32.const 0) (i32.const 0) (i32.const 8) (i32.const 8)
                        (f32.const 64) (f32.const 0) (f32.const 8) (f32.const 8)
                        (i32.const 0) (i32.const 0))
            (call $ovalo (f32.const 20) (f32.const 20) (f32.const 28) (f32.const 28)
                         (i32.const 7))
            (call $palr)
            (call $oval (f32.const 0) (f32.const 0) (f32.const 8) (f32.const 8) (i32.const 8))))
    "#;

    fn load_test_vm(wat_src: &str) -> Result<GameVm> {
        let wasm = wat::parse_str(wat_src).unwrap();
        GameVm::load(&wasm, &Assets::default(), AudioHandle::dummy())
    }

    #[test]
    fn parity_imports_link_and_run() {
        let mut vm = load_test_vm(PARITY_CART).unwrap();
        vm.call_update().unwrap();
        vm.call_draw().unwrap();
        // sset wrote sprite-sheet pixel (0,0) = 9.
        assert_eq!(vm.state().sprites.get(0, 0), 9);
        // ellipse_fill drew color 8 after reset_palette, so no remap applies.
        assert_eq!(vm.state().fb.pget(4, 4), 8, "oval filled the box center");
    }

    #[test]
    fn abi_lifecycle_and_drawing() {
        let mut vm = load_test_vm(TEST_CART).unwrap();
        assert_eq!(vm.state_mut().logs.pop().as_deref(), Some("hi from cart"));

        vm.call_update().unwrap();
        vm.call_draw().unwrap();
        assert_eq!(vm.state().fb.pget(5, 7), 8, "set_pixel through ABI");
        assert_eq!(vm.state().fb.pget(0, 0), 7, "print drew a glyph pixel");

        // Hold right; update should move the pixel.
        vm.state_mut().input.set_button(1, true);
        vm.call_update().unwrap();
        vm.call_draw().unwrap();
        assert_eq!(
            vm.state().fb.pget(6, 7),
            8,
            "is_button_down(right) moved pixel"
        );
    }

    #[test]
    fn default_fps_is_60() {
        // A cart with no rico8_fps export (e.g. hand-written WAT) takes the
        // default rate.
        let vm = load_test_vm(TEST_CART).unwrap();
        assert_eq!(vm.fps(), 60);
    }

    #[test]
    fn cart_can_select_30fps() {
        let vm = load_test_vm(FPS30_CART).unwrap();
        assert_eq!(vm.fps(), 30);
    }

    #[test]
    fn infinite_loop_is_trapped() {
        let mut vm = load_test_vm(LOOPING_CART).unwrap();
        let err = vm.call_update().unwrap_err();
        assert_eq!(err.phase, "update");
        assert!(err.message.contains("ran too long"), "{}", err.message);
    }

    #[test]
    fn missing_exports_is_a_load_error() {
        let wasm = wat::parse_str("(module)").unwrap();
        let err = match GameVm::load(&wasm, &Assets::default(), AudioHandle::dummy()) {
            Err(e) => e,
            Ok(_) => panic!("empty module should not load"),
        };
        assert!(err.to_string().contains("rico8_init"));
    }

    #[test]
    fn unknown_imports_are_rejected() {
        let wasm = wat::parse_str(
            r#"(module (import "env" "evil" (func))
                 (func (export "rico8_init"))
                 (func (export "rico8_update"))
                 (func (export "rico8_draw")))"#,
        )
        .unwrap();
        assert!(GameVm::load(&wasm, &Assets::default(), AudioHandle::dummy()).is_err());
    }

    #[test]
    fn fuel_budget_allows_modest_work() {
        let mut vm = load_test_vm(BUDGET_OK_CART).unwrap();
        assert!(
            vm.call_update().is_ok(),
            "10k-iteration frame must fit the 128K-fuel budget"
        );
    }

    #[test]
    fn fuel_budget_traps_runaway_work() {
        let mut vm = load_test_vm(BUDGET_OVER_CART).unwrap();
        let err = vm.call_update().unwrap_err();
        assert!(err.message.contains("ran too long"), "got: {}", err.message);
    }

    #[test]
    fn memory_growth_up_to_cap_is_allowed() {
        let mut vm = load_test_vm(GROW_TO_CAP_CART).unwrap();
        assert!(
            vm.call_update().is_ok(),
            "growing to exactly 128 K must succeed"
        );
    }

    #[test]
    fn memory_growth_past_cap_is_a_friendly_error() {
        let mut vm = load_test_vm(GROW_PAST_CAP_CART).unwrap();
        let err = vm.call_update().unwrap_err();
        assert!(
            err.message.contains("out of memory"),
            "got: {}",
            err.message
        );
    }

    #[test]
    fn oversized_initial_memory_is_rejected_at_load() {
        let wasm = wat::parse_str(HUGE_INITIAL_MEMORY_CART).unwrap();
        let err = match GameVm::load(&wasm, &Assets::default(), AudioHandle::dummy()) {
            Err(e) => e,
            Ok(_) => panic!("oversized cart should not load"),
        };
        assert!(err.to_string().contains("128K of memory"), "got: {err}");
    }
}
