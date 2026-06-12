# Web export

```text
rico8 export-web <project-dir|cart.png> <out.html>   # headless
> export mygame.html                                 # from the console
```

Either produces **one self-contained HTML file**. No server, no asset
folder, no build step for the player of the page — send the file,
double-click it, play. Opening it shows the cartridge art; clicking
boots the cart (the click also satisfies the browser's autoplay rule,
so audio just works).

## How it works

The page embeds two base64 payloads:

- **the cart PNG** — the exact same cartridge `export` produces (also
  used as the boot screen image), and
- **the browser player** — the `rico8-web` crate: `rico8-runtime`
  (rasterizer, font, palette, wasmi sandbox, synthesizer, cart codec)
  compiled to `wasm32-unknown-unknown` behind a tiny C-like export
  surface (`rico8_web_load`, `rico8_web_tick`, `rico8_web_fb_ptr`, …).

The page's JavaScript is deliberately as thin as the desktop's wgpu
layer: decode base64, instantiate the player (it imports nothing),
hand it the cart bytes, then per frame blit the RGBA framebuffer to a
128x128 canvas (CSS-scaled with `image-rendering: pixelated`), map
key events to the six buttons, and feed synth samples to WebAudio via
a short scheduled-buffer queue.

Yes, the cart's wasm runs in wasmi *inside* the player's wasm. The
double interpretation costs far less than a 128x128/30 fps cart can
spend, and it buys bit-identical behavior with the desktop console —
same rasterizer, same font, same synth, same error screens.

## Presentation

Black page, cartridge art, "click to play", pixel-perfect integer-ish
scaling, title underneath. Runtime errors show the same friendly
RICO-8 error screen as the desktop console, drawn by the player
itself. Keys match the desktop: arrows + `Z`/`X` (also `C`/`V`,
`N`/`M`).

On touch devices (detected via `pointer: coarse`) the page shows
on-screen controls under the canvas, PICO-8-web style: a d-pad on the
left and round `O`/`X` buttons on the right. The d-pad is one touch
zone with 8-way sectors, so diagonals and sliding between directions
work, and multi-touch lets you hold a direction while tapping a
button.

## Limitations

- **Play only.** No console, no editors, no `Esc` to the prompt. Web
  pages are players, not consoles.
- **File size.** The embedded player weighs ~1.2 MB (wasmi and the
  runtime), so every export is ~1.7 MB regardless of cart size.
- **Audio latency.** The scheduled-buffer queue adds ~100 ms; fine for
  jingles and blips, noticeable for rhythm games.
- **Source is not included.** Web exports embed a playable cart only;
  ship the `.png` cart alongside if you want people to `import` it.
- **Exporting needs the RICO-8 source tree** (the player is compiled
  on first export; afterwards it's cached). Installed binaries can
  point elsewhere with `RICO8_WEB=/path/to/rico8-web`.
- **No state saving.**
