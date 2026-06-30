# RICO-8 clipboard format

RICO-8's editors copy assets to the system clipboard as a plain-text blob:

    [rico8]<hex>[/rico8]

`<hex>` is the lowercase hex encoding of:

- `RICO8C` — six-byte magic (distinct from the on-disk assets magic `RICO8A`)
- one version byte (`0x01`)
- a [postcard](https://docs.rs/postcard)-encoded payload

The payload is a tagged union with four variants:

| Kind    | Fields                                                                        |
|---------|-------------------------------------------------------------------------------|
| Sprite  | width, height, one palette index per pixel, one flag byte per 8×8 sprite     |
| SFX     | source slot + the full SFX (including any drawn custom waveform)              |
| Pattern | a music pattern + the SFX each of its channels references (as slot + SFX)    |
| Map     | width, height, one 8-bit tile index per cell                                  |

All dimensions are in their natural units: Sprite width/height are in pixels, Map
width/height are in tiles. All data is row-major.

Because the payload reuses the on-disk asset structs directly, a copy is lossless:
sprite flags, custom waveforms, and 8-bit map tiles all survive the round-trip.

## Pasting

Paste accepts two formats:

**Native `[rico8]`** — decoded by checking the `RICO8C` magic and version byte, then
deserialising the postcard body. The full payload is restored, including sprite flags,
custom waveforms, and map regions.

**PICO-8 editor formats** — for interoperability, RICO-8 also parses PICO-8's
clipboard blobs:
- `[gfx]` — sprite pixels only; no sprite flags are carried.
- `[sfx]` — SFX records and song patterns, without custom waveforms.

PICO-8 has no map clipboard format, so map regions can only be transferred via the
native format. Any unrecognised or malformed blob is ignored.

## Validation

On decode, RICO-8 checks the `RICO8C` magic, the exact version byte, and
that the postcard body deserialises cleanly. Any mismatch is an error; the paste
is a no-op. Readers must reject versions they do not know.

## Versioning policy

The version byte covers the postcard schema. Any change to a variant's fields
requires a version bump so old consoles can reject new blobs cleanly rather than
silently misinterpret them.
