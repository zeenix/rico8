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
        write!(f, "runtime error in {}:\n{}", self.phase, self.message)
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
        let mut config = Config::default();
        config.consume_fuel(true);
        let engine = Engine::new(&config);
        let module = Module::new(&engine, wasm).map_err(|e| anyhow!("invalid cart wasm: {e}"))?;

        audio.load(assets.sfx.clone(), assets.music.clone());
        let mut store = Store::new(&engine, HostState::new(assets, audio));
        store.limiter(|state| &mut state.limits);
        let mut linker = <Linker<HostState>>::new(&engine);

        link!(linker, "cls", |mut c: Caller<'_, HostState>, col: i32| {
            c.data_mut().fb.cls(col as u8)
        });
        link!(linker, "camera", |mut c: Caller<'_, HostState>,
                                 x: i32,
                                 y: i32| {
            c.data_mut().fb.camera(x, y)
        });
        link!(linker, "clip", |mut c: Caller<'_, HostState>,
                               x: i32,
                               y: i32,
                               w: i32,
                               h: i32| {
            c.data_mut().fb.clip(x, y, w, h)
        });
        link!(linker, "pset", |mut c: Caller<'_, HostState>,
                               x: i32,
                               y: i32,
                               col: i32| {
            c.data_mut().fb.pset(x, y, col as u8)
        });
        link!(linker, "pget", |c: Caller<'_, HostState>,
                               x: i32,
                               y: i32|
         -> i32 {
            c.data().fb.pget(x, y) as i32
        });
        link!(linker, "line", |mut c: Caller<'_, HostState>,
                               x0: i32,
                               y0: i32,
                               x1: i32,
                               y1: i32,
                               col: i32| {
            c.data_mut().fb.line(x0, y0, x1, y1, col as u8)
        });
        link!(linker, "rect", |mut c: Caller<'_, HostState>,
                               x0: i32,
                               y0: i32,
                               x1: i32,
                               y1: i32,
                               col: i32| {
            c.data_mut().fb.rect(x0, y0, x1, y1, col as u8)
        });
        link!(
            linker,
            "rectfill",
            |mut c: Caller<'_, HostState>, x0: i32, y0: i32, x1: i32, y1: i32, col: i32| {
                c.data_mut().fb.rectfill(x0, y0, x1, y1, col as u8)
            }
        );
        link!(linker, "circ", |mut c: Caller<'_, HostState>,
                               x: i32,
                               y: i32,
                               r: i32,
                               col: i32| {
            c.data_mut().fb.circ(x, y, r, col as u8)
        });
        link!(
            linker,
            "circfill",
            |mut c: Caller<'_, HostState>, x: i32, y: i32, r: i32, col: i32| {
                c.data_mut().fb.circfill(x, y, r, col as u8)
            }
        );
        link!(linker, "print", |mut c: Caller<'_, HostState>,
                                ptr: u32,
                                len: u32,
                                x: i32,
                                y: i32,
                                col: i32|
         -> i32 {
            let s = read_guest_str(&c, ptr, len);
            c.data_mut().fb.print(&s, x, y, col as u8)
        });
        link!(linker, "btn", |c: Caller<'_, HostState>, b: u32| -> i32 {
            c.data().input.btn(b) as i32
        });
        link!(linker, "btnp", |c: Caller<'_, HostState>, b: u32| -> i32 {
            c.data().input.btnp(b) as i32
        });
        link!(linker, "spr", |mut c: Caller<'_, HostState>,
                              n: u32,
                              x: i32,
                              y: i32,
                              w: u32,
                              h: u32,
                              flip_x: i32,
                              flip_y: i32| {
            let HostState { fb, sprites, .. } = c.data_mut();
            fb.spr(sprites, n, x, y, w, h, flip_x != 0, flip_y != 0);
        });
        link!(linker, "map", |mut c: Caller<'_, HostState>,
                              cel_x: i32,
                              cel_y: i32,
                              sx: i32,
                              sy: i32,
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
        link!(linker, "mget", |c: Caller<'_, HostState>,
                               x: i32,
                               y: i32|
         -> i32 {
            c.data().map.get(x, y) as i32
        });
        link!(linker, "mset", |mut c: Caller<'_, HostState>,
                               x: i32,
                               y: i32,
                               v: u32| {
            c.data_mut().map.set(x, y, v as u8)
        });
        link!(linker, "fget", |c: Caller<'_, HostState>, n: u32| -> i32 {
            c.data().sprites.flags(n) as i32
        });
        link!(linker, "fset", |mut c: Caller<'_, HostState>,
                               n: u32,
                               flags: u32| {
            c.data_mut().sprites.flags[(n as usize) % crate::assets::SPRITE_COUNT] = flags as u8;
        });
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

        store
            .set_fuel(FUEL_PER_CALL)
            .map_err(|e| anyhow!("fuel setup: {e}"))?;
        let instance = linker
            .instantiate_and_start(&mut store, &module)
            .map_err(|e| {
                let s = e.to_string();
                if s.contains("resource limiter denied") {
                    anyhow!("cart needs more than 128K of memory to start")
                } else {
                    anyhow!("cart does not match the RICO-8 ABI: {e}")
                }
            })?;

        let init = instance
            .get_typed_func::<(), ()>(&store, "rico8_init")
            .map_err(|e| anyhow!("cart is missing rico8_init: {e}"))?;
        let update = instance
            .get_typed_func::<(), ()>(&store, "rico8_update")
            .map_err(|e| anyhow!("cart is missing rico8_update: {e}"))?;
        let draw = instance
            .get_typed_func::<(), ()>(&store, "rico8_draw")
            .map_err(|e| anyhow!("cart is missing rico8_draw: {e}"))?;

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
          (import "rico8" "cls" (func $cls (param i32)))
          (import "rico8" "pset" (func $pset (param i32 i32 i32)))
          (import "rico8" "pget" (func $pget (param i32 i32) (result i32)))
          (import "rico8" "btn" (func $btn (param i32) (result i32)))
          (import "rico8" "print" (func $print (param i32 i32 i32 i32 i32) (result i32)))
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
            (call $pset (global.get $x) (i32.const 7) (i32.const 8))
            (drop (call $print (i32.const 16) (i32.const 2) (i32.const 0) (i32.const 0) (i32.const 7))))
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

    fn load_test_vm(wat_src: &str) -> Result<GameVm> {
        let wasm = wat::parse_str(wat_src).unwrap();
        GameVm::load(&wasm, &Assets::default(), AudioHandle::dummy())
    }

    #[test]
    fn abi_lifecycle_and_drawing() {
        let mut vm = load_test_vm(TEST_CART).unwrap();
        assert_eq!(vm.state_mut().logs.pop().as_deref(), Some("hi from cart"));

        vm.call_update().unwrap();
        vm.call_draw().unwrap();
        assert_eq!(vm.state().fb.pget(5, 7), 8, "pset through ABI");
        assert_eq!(vm.state().fb.pget(0, 0), 7, "print drew a glyph pixel");

        // Hold right; update should move the pixel.
        vm.state_mut().input.set_button(1, true);
        vm.call_update().unwrap();
        vm.call_draw().unwrap();
        assert_eq!(vm.state().fb.pget(6, 7), 8, "btn(right) moved pixel");
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
