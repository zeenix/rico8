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
| `__map__`                 | map (all 64 rows)       | see the map note below                   |
| `__label__`               | cart label             | the captured screenshot, when present    |
| `__sfx__`                 | sound effects           | notes, speed, loop points                |
| `__music__`               | music patterns          | channels and loop/stop flags             |

Because the palette and the audio model line up exactly — waveforms
(triangle, tilted saw, saw, square, pulse, organ, noise, phaser) and effects
(slide, vibrato, drop, fade in, fade out, arpeggios) are in the same order —
sprites *look* and sounds *sound* like they did in PICO-8. SFX that use
another SFX as a **custom instrument** carry that across too, the same way
PICO-8 packs it (waveform nibble bit 3); the SFX editor shows custom-instrument
steps in yellow and `i` toggles the flag.

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

## The shared region

A PICO-8 map is 128x32 tiles by default; its lower half *optionally* aliases
the bottom half of the sprite sheet (`0x1000–0x1fff`). A cart uses that memory
for one or the other, and nothing in the file says which. RICO-8 de-aliases
the two — it has a full 256-sprite sheet **and** a full 128x64 map — so the
import simply brings the shared region across **both** ways: it fills sprites
128–255 *and* map rows 32–63 from the same bytes. Keep whichever your cart
actually used and clear the other in the editor.

## Limitations

- **Custom instruments are approximated.** The custom-instrument flag and the
  referenced SFX are preserved, and playback borrows that SFX's timbre (its
  first step's waveform) at the played pitch — close, but not a sample-exact
  reproduction of PICO-8's instrument synthesis.
- **The code, as above.** Assets transfer; the logic is yours to write.
