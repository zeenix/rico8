# Importing PICO-8 cartridges

RICO-8 is a homage to [PICO-8](https://www.lexaloffle.com/pico-8.php), and
it shows in the formats: the two consoles share the exact same 16-color
palette, the same eight chip-tune waveforms and per-step effects, and the
same 128x128 sprite sheet laid out 16 sprites to a row. That overlap makes a
PICO-8 cart's **assets** import into a RICO-8 project almost one-to-one.

```text
rico8 import-pico8 <cart.p8|cart.p8.png> <dir>
```

or, from the console prompt:

```text
> import-pico8 celeste.p8 mycart
```

This creates `./<dir>` as a normal RICO-8 project (a real Cargo crate),
fills `assets.rico8` with the imported graphics, and drops you into it ready
to edit.

## What transfers

| PICO-8 section            | RICO-8 asset            | Notes                                    |
| ------------------------- | ----------------------- | ---------------------------------------- |
| `__gfx__` (sprite sheet)  | sprite sheet            | 128x128, one palette index per pixel     |
| `__gff__` (sprite flags)  | sprite flags            | all 256 sprites, 8 flags each            |
| `__map__`                 | map (top 32 rows)       | see the map note below                   |
| `__label__`               | cart label             | the captured screenshot, when present    |
| `__sfx__`                 | sound effects           | notes, speed, loop points                |
| `__music__`               | music patterns          | channels and loop/stop flags             |

Because the palette and the audio model line up exactly — waveforms
(triangle, tilted saw, saw, square, pulse, organ, noise, phaser) and effects
(slide, vibrato, drop, fade in, fade out, arpeggios) are in the same order —
sprites *look* and sounds *sound* like they did in PICO-8.

## What does not: the code

Only the assets are imported. PICO-8 games are written in Lua and RICO-8
games in Rust, so the cart's code is **ignored** entirely — for both formats.
The new project gets a small stub `src/lib.rs` that builds immediately and
draws a placeholder; write your game logic in Rust there, against the
imported art and audio.

## The two formats

- **`.p8`** — the plain-text cartridge: `__gfx__`, `__gff__`, `__label__`,
  `__map__`, `__sfx__` and `__music__` sections of hex (the `__lua__` code
  section is skipped).
- **`.p8.png`** — the PNG cartridge: the game's 32 KiB ROM is hidden two
  bits at a time in the low bits of each pixel's alpha/red/green/blue
  channels. RICO-8 decodes the 160x205 image, reassembles the ROM, and reads
  it using the fixed PICO-8 memory map (`0x0000` gfx, `0x2000` map, `0x3000`
  flags, `0x3100` music, `0x3200` sfx).

## Limitations

- **Map sharing.** A PICO-8 map is 128x32 tiles by default; its lower half
  optionally aliases the shared sprite memory. The import brings the 32
  explicit rows into the top of RICO-8's 128x64 map and leaves the rest
  empty. If a cart used the shared region for map data, those tiles stay in
  the sprite sheet (sprites 128–255), exactly as PICO-8 stored them.
- **Custom instruments.** PICO-8 SFX that reference other SFX as custom
  instruments are imported as their base built-in waveform (0–7); the custom
  layering is dropped.
- **The code, as above.** Assets transfer; the logic is yours to write.
