# The RICO-8 PNG cartridge format (version 1)

A RICO-8 cart is a valid PNG image. The picture is the cartridge: body,
color stripes, a 128x128 label (the cart's captured screenshot, or the
built-in RICO-8 art), title and author. The game itself rides inside a
private ancillary chunk, so any image tool sees a normal 160x205 PNG
and RICO-8 sees a game.

## Layout

```text
PNG signature
IHDR                 160 x 205, 8-bit RGBA
IDAT                 the cartridge art (zlib, filter 0)
rcRt                 the RICO-8 payload chunk
IEND
```

`rcRt` chunk-type properties, per the PNG spec: ancillary (`r`),
private (`c`), reserved bit valid (`R`), safe-to-copy (`t`) — image
editors that don't know about RICO-8 are allowed to pass it through.

## The `rcRt` chunk

```text
offset  size  field
0       5     magic: "RICO8"
5       2     format version, u16 little-endian (currently 1)
7       ...   DEFLATE-compressed body
```

The body is a [postcard](https://docs.rs/postcard)-encoded `Cart`:

```rust
struct Cart {
    wasm: Vec<u8>,          // compiled wasm32-unknown-unknown module
    assets: Assets,         // sprites, map, sfx, music, metadata, label
    source: Option<String>, // src/lib.rs, present in editable carts
}
```

The whole body is compressed as one stream (wasm + assets + source
together), which is why including the source usually costs little.

### Playable vs editable

- **playable cart** — `source: None`. Loads and runs; `import` refuses.
- **editable cart** — `source: Some(...)`. `rico8 extract cart.png dir`
  (or `import` in the console) reconstructs a buildable project from it.

Both are produced by `export`; pass `-nosrc` (console) or
`--no-source` (CLI) for a playable-only cart.

## Validation

On load, RICO-8 checks: PNG signature, `rcRt` CRC, magic, exact format
version, a decompression size cap (64 MiB), the `\0asm` wasm magic, and
asset dimensions (sprite sheet 128x128, 256 flags, map 128x64, 64 SFX,
64 patterns, label 128x128 if present). Anything off is a load error,
never undefined behavior.

## Versioning policy

The version is the contract for *everything inside*: payload schema,
asset dimensions and the ABI the wasm was built against. Readers must
reject versions they don't know. Compatible additions still bump the
version; old consoles refusing new carts beats silently broken games.
