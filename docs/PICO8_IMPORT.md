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

| PICO-8 section           | RICO-8 asset      | Notes                                 |
| ------------------------ | ----------------- | ------------------------------------- |
| `__gfx__` (sprite sheet) | sprite sheet      | 128x128, one palette index per pixel  |
| `__gff__` (sprite flags) | sprite flags      | all 256 sprites, 8 flags each         |
| `__map__`                | map (all 64 rows) | see the map note below                |
| `__label__`              | cart label        | the captured screenshot, when present |
| `__sfx__`                | sound effects     | notes, speed, loop points, filters    |
| `__music__`              | music patterns    | channels and loop/stop flags          |

Because the palette and the audio model line up exactly — waveforms
(triangle, tilted saw, saw, square, pulse, organ, noise, phaser) and effects
(slide, vibrato, drop, fade in, fade out, arpeggios) are in the same order —
sprites *look* and sounds *sound* like they did in PICO-8. SFX that use
another SFX as a **custom instrument** carry that across too, the same way
PICO-8 packs it (waveform nibble bit 3); the SFX editor shows custom-instrument
steps in yellow and `i` toggles the flag.

The per-SFX **filter switches** — noiz, buzz, detune, reverb and dampen — come
across as well (PICO-8 packs them into the SFX's filter byte). They appear as a
`nz bz dt rv dm` strip on the right of the SFX editor; click to toggle the
on/off pair and cycle the three levelled ones.

## What does not: the code

Only the assets are imported. PICO-8 games are written in Lua and RICO-8
games in Rust, so the cart's code is **ignored** entirely — for both formats.
The new project gets a small stub `src/lib.rs` that builds immediately and
draws a placeholder; write your game logic in Rust there, against the
imported art and audio.

## Selective, additive import

The whole-cart `import-pico8` above makes a *new* project. To pull only some
assets into an **existing** project, use `--into`:

```text
rico8 import-pico8 <cart.p8|.p8.png> --into <project-dir> [--sprites R] [--sfx R] [--music R]
```

or, from the console prompt with a project loaded:

```text
> import-pico8 <cart> --into --sprites 0-15,32 --sfx 0-3
```

Each `R` is a comma-separated list of indices and inclusive ranges, e.g.
`0-15,32,40-43`. At least one of `--sprites`, `--sfx`, `--music` is required.

The import is **additive**: each kind's items are appended immediately after the
destination's last used slot, so nothing already in the cart is overwritten or
renumbered. Sprites bring their flag bytes along.

Because imported items land at different slot numbers than in the source, references
**within a single import** are remapped: imported music patterns are repointed at
wherever their imported SFX landed. A reference to a slot you did not select is
left as-is and reported as a warning — for example, importing music without the
SFX it plays. One limit carries over from PICO-8: a note's custom-instrument
reference is only three bits wide (slots 0-7), so an imported custom instrument
that lands past slot 7 cannot be repointed; it is left as-is and a warning is
printed.

Map tiles and the cart label are not part of selective import.

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

- **Some audio is approximated.** Every audio value is preserved and round-
  trips, but two things are modelled rather than reproduced bit-for-bit:
  custom instruments borrow the referenced SFX's timbre (its first step's
  waveform) at the played pitch, and the filter switches (noiz/buzz/detune/
  reverb/dampen) are faithful approximations of PICO-8's, not its exact DSP.
- **The code, as above.** Assets transfer; the logic is yours to write.

## Pasting from PICO-8

Besides importing a whole cart, you can copy individual assets out of PICO-8 and paste them
straight into a RICO-8 editor. In PICO-8, select sprites, a range of SFX, or a music pattern and
press **Ctrl+C**; switch to the matching RICO-8 editor and press **Ctrl+V**.

- **Sprites** (sprite editor) paste as a rectangle with its top-left at the selected sprite.
  Anything past the edge is clipped.
- **SFX** (SFX editor) overwrite consecutive slots starting at the selected one; a note that
  plays another copied SFX as a custom instrument is repointed to where that SFX lands.
- **Music patterns** are a special case: PICO-8 copies a pattern as its SFX. Pasted in the
  music editor, RICO-8 rebuilds the pattern at the selected slot and appends its SFX after the
  last used SFX slot, repointing the pattern's channels to them — your existing SFX are left
  alone.

The bottom bar reports what happened (e.g. `pasted 4 SFX 3-6`), including a count if something
did not fit or a reference could not be resolved. Pasting the wrong kind for an editor shows a
hint naming the right one.
