//! Web export: turn a cart into a single self-contained HTML file.
//!
//! The file embeds two things, base64-encoded: the browser player (the
//! console runtime compiled to wasm, from the `rico8-web` crate) and
//! the cart PNG itself. No server, no sidecar files — double-click the
//! HTML and the cart boots, PICO-8-web style: cartridge art first,
//! click to play. See docs/WEB_EXPORT.md for the details and limits.

use anyhow::{anyhow, Context, Result};
use rico8_runtime::cart::{self, Cart};
use std::path::{Path, PathBuf};
use std::process::Command;

/// Export `cart` as a playable single-file HTML page.
pub fn export_html(cart: &Cart, out: &Path, web_crate_dir: &Path) -> Result<()> {
    let player_wasm = build_player(web_crate_dir)?;
    let cart_png = cart::encode(cart)?;
    let title = if cart.assets.meta.name.is_empty() {
        "rico-8 cart".to_string()
    } else {
        cart.assets.meta.name.clone()
    };
    let html = TEMPLATE
        .replace("{{TITLE}}", &escape_html(&title))
        .replace("{{PLAYER_B64}}", &base64(&player_wasm))
        .replace("{{CART_B64}}", &base64(&cart_png));
    std::fs::write(out, html)?;
    Ok(())
}

/// Where the `rico8-web` player crate lives. Defaults to this source
/// tree; override with RICO8_WEB for installed binaries.
pub fn web_crate_dir(sdk_path: &Path) -> PathBuf {
    if let Ok(p) = std::env::var("RICO8_WEB") {
        return PathBuf::from(p);
    }
    sdk_path.join("../rico8-web")
}

/// Compile the browser player to wasm (a fast no-op after the first
/// time) and return its bytes.
fn build_player(web_crate_dir: &Path) -> Result<Vec<u8>> {
    let output = Command::new("cargo")
        .args([
            "build",
            "--profile",
            "web-release",
            "--target",
            "wasm32-unknown-unknown",
        ])
        .current_dir(web_crate_dir)
        .env("CARGO_TERM_COLOR", "never")
        .output()
        .context("running cargo for the web player")?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let tail: Vec<&str> = stderr.lines().rev().take(8).collect();
        return Err(anyhow!(
            "building the web player failed:\n{}",
            tail.into_iter().rev().collect::<Vec<_>>().join("\n")
        ));
    }
    let target_dir = std::env::var("CARGO_TARGET_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| web_crate_dir.join("../target"));
    let artifact = target_dir.join("wasm32-unknown-unknown/web-release/rico8_web.wasm");
    std::fs::read(&artifact)
        .with_context(|| format!("reading web player at {}", artifact.display()))
}

fn escape_html(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

/// Plain standard base64; small enough to not warrant a dependency.
fn base64(data: &[u8]) -> String {
    const CHARS: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(data.len().div_ceil(3) * 4);
    for chunk in data.chunks(3) {
        let b = [
            chunk[0],
            *chunk.get(1).unwrap_or(&0),
            *chunk.get(2).unwrap_or(&0),
        ];
        let n = u32::from_be_bytes([0, b[0], b[1], b[2]]);
        out.push(CHARS[(n >> 18 & 63) as usize] as char);
        out.push(CHARS[(n >> 12 & 63) as usize] as char);
        out.push(if chunk.len() > 1 {
            CHARS[(n >> 6 & 63) as usize] as char
        } else {
            '='
        });
        out.push(if chunk.len() > 2 {
            CHARS[(n & 63) as usize] as char
        } else {
            '='
        });
    }
    out
}

/// The wrapper page. Deliberately spartan: black page, cartridge art,
/// click to boot, pixel-perfect canvas. No frameworks, no fetches.
const TEMPLATE: &str = r#"<!doctype html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1, maximum-scale=1, user-scalable=no">
<title>{{TITLE}} - rico-8</title>
<style>
  html, body { margin: 0; height: 100%; background: #000; }
  body { display: flex; flex-direction: column; align-items: center;
         justify-content: center; gap: 12px;
         font-family: monospace; color: #5f574f; }
  #stage { position: relative; width: min(85vmin, 512px);
           aspect-ratio: 1; }
  canvas, #boot { position: absolute; inset: 0; width: 100%; height: 100%; }
  canvas { image-rendering: pixelated; image-rendering: crisp-edges;
           display: none; }
  #boot { display: flex; flex-direction: column; align-items: center;
          justify-content: center; gap: 16px; cursor: pointer;
          border: 0; background: none; padding: 0; }
  #boot img { height: 80%; image-rendering: pixelated; }
  #boot span { color: #fff1e8; font-size: 16px; }
  #boot:hover span { color: #ffec27; }
  #title { color: #c2c3c7; font-size: 14px; }
  #hint { font-size: 11px; }
  a { color: #5f574f; }

  /* Touch controls: hidden unless the device has a coarse pointer. */
  #touch { display: none; width: 100%; max-width: 560px;
           justify-content: space-between; align-items: center;
           padding: 8px 18px; box-sizing: border-box;
           user-select: none; -webkit-user-select: none; }
  @media (pointer: coarse) {
    body { justify-content: flex-start; padding-top: 10px;
           overscroll-behavior: none; }
    #stage { width: min(92vmin, 56vh); }
    #touch { display: flex; touch-action: none; }
    #hint { display: none; }
  }
  #dpad { position: relative; width: 34vmin; height: 34vmin;
          max-width: 180px; max-height: 180px; }
  #dpad::before, #dpad::after { content: ""; position: absolute;
          background: #1d2b53; border: 2px solid #5f574f;
          box-sizing: border-box; border-radius: 6px; }
  #dpad::before { left: 33%; top: 0; width: 34%; height: 100%; }
  #dpad::after { left: 0; top: 33%; width: 100%; height: 34%; }
  #dpad .dir { position: absolute; color: #5f574f; font-size: 18px;
          z-index: 1; transform: translate(-50%, -50%); }
  #dpad .dir.on { color: #ffec27; }
  #d-l { left: 16%; top: 50%; } #d-r { left: 84%; top: 50%; }
  #d-u { left: 50%; top: 16%; } #d-d { left: 50%; top: 84%; }
  #abtns { display: flex; gap: 14px; align-items: flex-end; }
  .ab { width: 17vmin; height: 17vmin; max-width: 90px; max-height: 90px;
        border-radius: 50%; border: 2px solid #5f574f;
        background: #1d2b53; color: #c2c3c7; font-family: monospace;
        font-size: 24px; padding: 0; }
  #btn-o { margin-bottom: 26px; }
  .ab.on { background: #7e2553; color: #fff1e8; border-color: #ff77a8; }
</style>
</head>
<body>
<div id="stage">
  <canvas id="screen" width="128" height="128"></canvas>
  <button id="boot"><img alt="cartridge" id="cartimg"><span>click to play</span></button>
</div>
<div id="touch">
  <div id="dpad">
    <span class="dir" id="d-l">&#9664;</span><span class="dir" id="d-r">&#9654;</span>
    <span class="dir" id="d-u">&#9650;</span><span class="dir" id="d-d">&#9660;</span>
  </div>
  <div id="abtns">
    <button class="ab" id="btn-o">o</button>
    <button class="ab" id="btn-x">x</button>
  </div>
</div>
<div id="title">{{TITLE}}</div>
<div id="hint">arrows + z/x &middot; made with <a href="https://github.com/zeenix/rico8">rico-8</a></div>
<script>
"use strict";
const PLAYER_B64 = "{{PLAYER_B64}}";
const CART_B64 = "{{CART_B64}}";
const FPS = 30, SCREEN = 128, SAMPLE_RATE = 44100;

function b64bytes(b64) {
  const s = atob(b64);
  const a = new Uint8Array(s.length);
  for (let i = 0; i < s.length; i++) a[i] = s.charCodeAt(i);
  return a;
}

document.getElementById("cartimg").src = "data:image/png;base64," + CART_B64;

const canvas = document.getElementById("screen");
const ctx2d = canvas.getContext("2d");
const image = new ImageData(SCREEN, SCREEN);

// Same physical keys as the desktop console.
const KEYMAP = {
  ArrowLeft: 0, ArrowRight: 1, ArrowUp: 2, ArrowDown: 3,
  KeyZ: 4, KeyC: 4, KeyN: 4, KeyX: 5, KeyV: 5, KeyM: 5,
};

let wasm = null;
let audioCtx = null;
let audioTime = 0;
let last = 0, acc = 0;

async function boot() {
  document.getElementById("boot").style.display = "none";
  canvas.style.display = "block";

  const { instance } =
    await WebAssembly.instantiate(b64bytes(PLAYER_B64), {});
  wasm = instance.exports;

  const cart = b64bytes(CART_B64);
  const ptr = wasm.rico8_web_upload_begin(cart.length);
  new Uint8Array(wasm.memory.buffer, ptr, cart.length).set(cart);
  if (wasm.rico8_web_load() !== 0) {
    const msg = new TextDecoder().decode(new Uint8Array(
      wasm.memory.buffer, wasm.rico8_web_error_ptr(), wasm.rico8_web_error_len()));
    document.getElementById("title").textContent = "cart error: " + msg;
    return;
  }

  addEventListener("keydown", (e) => key(e, 1));
  addEventListener("keyup", (e) => key(e, 0));

  audioCtx = new (window.AudioContext || window.webkitAudioContext)();
  audioTime = 0;

  last = performance.now();
  requestAnimationFrame(frame);
}

function key(e, down) {
  const b = KEYMAP[e.code];
  if (b === undefined) return;
  e.preventDefault();
  wasm.rico8_web_set_button(b, down);
}

// --- Touch controls: d-pad + O/X, multi-touch, 8-way diagonals. ---
const touchState = [0, 0, 0, 0, 0, 0];
const el = (id) => document.getElementById(id);

function inRect(r, t, slop) {
  return t.clientX >= r.left - slop && t.clientX <= r.right + slop &&
         t.clientY >= r.top - slop && t.clientY <= r.bottom + slop;
}

function readTouches(e) {
  e.preventDefault();
  if (!wasm) return;
  const next = [0, 0, 0, 0, 0, 0];
  const pad = el("dpad").getBoundingClientRect();
  const ro = el("btn-o").getBoundingClientRect();
  const rx = el("btn-x").getBoundingClientRect();
  for (const t of e.touches) {
    if (inRect(ro, t, 12)) { next[4] = 1; continue; }
    if (inRect(rx, t, 12)) { next[5] = 1; continue; }
    if (!inRect(pad, t, pad.width * 0.3)) continue;
    const dx = t.clientX - (pad.left + pad.width / 2);
    const dy = t.clientY - (pad.top + pad.height / 2);
    if (Math.hypot(dx, dy) < pad.width * 0.1) continue; // dead zone
    // 8-way: overlapping 135-degree sectors make 45-degree diagonals.
    const a = Math.atan2(dy, dx) * 180 / Math.PI;
    if (Math.abs(a) < 67.5) next[1] = 1;          // right
    if (Math.abs(a) > 112.5) next[0] = 1;         // left
    if (a < -22.5 && a > -157.5) next[2] = 1;     // up
    if (a > 22.5 && a < 157.5) next[3] = 1;       // down
  }
  const vis = ["d-l", "d-r", "d-u", "d-d", "btn-o", "btn-x"];
  for (let b = 0; b < 6; b++) {
    if (next[b] !== touchState[b]) {
      touchState[b] = next[b];
      wasm.rico8_web_set_button(b, next[b]);
      el(vis[b]).classList.toggle("on", next[b] === 1);
    }
  }
}

for (const ev of ["touchstart", "touchmove", "touchend", "touchcancel"]) {
  el("touch").addEventListener(ev, readTouches, { passive: false });
}

// Keep a short queue of scheduled audio buffers ahead of the clock.
function pumpAudio() {
  if (!audioCtx) return;
  const now = audioCtx.currentTime;
  if (audioTime < now) audioTime = now + 0.05;
  while (audioTime < now + 0.15) {
    const n = wasm.rico8_web_audio_render(2048);
    if (n === 0) return;
    const samples = new Float32Array(
      wasm.memory.buffer, wasm.rico8_web_audio_ptr(), n);
    const buf = audioCtx.createBuffer(1, n, SAMPLE_RATE);
    buf.getChannelData(0).set(samples);
    const src = audioCtx.createBufferSource();
    src.buffer = buf;
    src.connect(audioCtx.destination);
    src.start(audioTime);
    audioTime += n / SAMPLE_RATE;
  }
}

function frame(now) {
  // Fixed 30 fps logic under a variable display rate.
  acc = Math.min(acc + (now - last), 200);
  last = now;
  const step = 1000 / FPS;
  while (acc >= step) {
    wasm.rico8_web_tick();
    acc -= step;
  }
  const ptr = wasm.rico8_web_fb_ptr();
  if (ptr !== 0) {
    image.data.set(new Uint8Array(wasm.memory.buffer, ptr, SCREEN * SCREEN * 4));
    ctx2d.putImageData(image, 0, 0);
  }
  pumpAudio();
  requestAnimationFrame(frame);
}

document.getElementById("boot").addEventListener("click", () => {
  boot().catch((e) => {
    document.getElementById("title").textContent = "boot failed: " + e;
  });
});
</script>
</body>
</html>
"#;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base64_matches_reference() {
        assert_eq!(base64(b""), "");
        assert_eq!(base64(b"f"), "Zg==");
        assert_eq!(base64(b"fo"), "Zm8=");
        assert_eq!(base64(b"foo"), "Zm9v");
        assert_eq!(base64(b"foobar"), "Zm9vYmFy");
        assert_eq!(base64(&[0xff, 0xef, 0xbe]), "/+++");
    }

    #[test]
    fn html_is_escaped() {
        assert_eq!(escape_html("a<b>&c"), "a&lt;b&gt;&amp;c");
    }
}
