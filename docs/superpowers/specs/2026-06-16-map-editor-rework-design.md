# Map editor rework — design

## Problem

The current map editor (`rico8-console/src/editor/map.rs`, ~119 lines) is a minimal
paint-one-tile tool. Concretely:

- No visual separation between the map grid and the sprite picker below it.
- The only operation is sprite placement — no cell selection, copy, or paste.
- No highlight around the hovered cell, and no per-cell info in the status bar.
- The picker is too small (2 rows = 32 sprites) and has no on-screen page selector, so most
  of the 256 sprites are hard to reach.

This rework brings the map editor up to the polish level of the sprite editor and mirrors
PICO-8's map-editor tooling, while staying within RICO-8's one-rasterizer, headless-testable
model.

## Scope

In scope: a reworked `map.rs`, a small shared-icon refactor in `rico8-console/src/ui.rs`, and a
one-icon upgrade to the sprite editor (the pencil). Out of scope: the asset model, the cart/ABI
surface, map dimensions, and any sprite-editor behaviour. The page selector stays as dots (to
match the sprite editor), not PICO-8's numbered tabs.

## Layout (128×128)

The shell owns the top tab bar (y 0–7) and overlays the cursor; the editor owns y 8–119 and
draws its own status bar at y 120–127 via `ui::status_bar`. Vertical budget for the body is
112 px, split as:

| Region        | y range  | Size            | Notes                                        |
|---------------|----------|-----------------|----------------------------------------------|
| Toolbar       | 8–17     | 10 px           | 6 tool icons (left); brush preview + 4 page dots (right) |
| Map view      | 18–81    | 64 px = 16×8    | scrollable viewport                          |
| Separator     | 82–83    | 2 px            | a horizontal divider line                    |
| Sprite sheet  | 84–115   | 32 px = 4 rows  | one full 64-sprite page (sprite-editor parity) |
| (gap)         | 116–119  | 4 px            | dark background                              |
| Status bar    | 120–127  | 8 px            | `ui::status_bar`                             |

Constants in `map.rs`:

```
VIEW_Y = 18; VIEW_TILES_X = 16; VIEW_TILES_Y = 8;
SEP_Y = 82; SHEET_Y = 84;            // 4 rows of 8 px
TOOLBAR_Y = 8;                       // icons drawn at y 9
```

Camera clamps: `cam_x` in `0..=MAP_W-16` (0–112), `cam_y` in `0..=MAP_H-8` (0–56)
(`MAP_W=128`, `MAP_H=64`).

### Toolbar

Six tool icons in a row starting at x 2, one per 9 px slot (8 px icon field + 1 px gap); the
7-or-8-px glyph is centred within its slot. The active tool gets a black background box (as the
sprite editor does). Right side: an 8×8 brush preview (the selected sprite drawn as-is) near
x 88, and four page dots at `x = 104 + p*6, y ≈ 11` (5×5 filled squares, active = white,
inactive = dark-purple), mirroring the sprite editor's `PAGE_BTNS`.

### Sprite sheet & paging

Four rows × 16 = one 64-sprite page, drawn by pixel-copy (`fb.pset`) exactly like the sprite
editor's sheet strip. Paging switches to **4 pages of 64** (was 8×32), so `n = page*64 + cy*16
+ cx` and the page count is `% 4` — consistent with the sprite editor. A white selection box
outlines the brush sprite when `brush/64 == page`. Clicking the strip sets the brush; clicking a
page dot sets the page; PageUp/PageDown cycle pages mod 4.

## Tools

A `Tool` enum in PICO-8's toolbar order, icons sampled pixel-for-pixel from PICO-8:

```
enum Tool { Draw, Paste, Select, Pan, Fill, Circle }
```

1. **Draw** (pencil) — left-button paints the brush into the hovered cell; drag-paints across
   cells. No right-button behaviour anywhere in the editor.
2. **Paste** (stamp-from-clipboard) — left-click stamps the clipboard block with its top-left at
   the hovered cell; repeatable. No-op if the clipboard is empty.
3. **Select** (marquee) — drag to define a rectangular selection of cells, rendered with an
   animated marching-ants border (see below). With a selection active: `Ctrl+C` copies the cells
   to the clipboard, `Ctrl+X` cuts (copy then clear source to 0), `Delete`/`Backspace` clears the
   cells to 0, and pressing inside the selection and dragging **moves** the block (lift source to
   0, write at the new offset; on overlap, destination wins). Dragging outside starts a new
   selection.
4. **Pan** (hand) — drag to scroll the viewport. Press records an anchor `(mouse_px, cam)`; on
   drag, `cam = anchor_cam + (anchor_mouse - cur_mouse) / 8`, clamped. Arrow keys still scroll.
5. **Fill** (bucket) — a click without drag flood-fills the contiguous region of cells equal to
   the clicked cell's tile, replacing it with the brush (stack-based, bounded to the map). A drag
   fills the dragged rectangle with the brush on release; a preview outline is shown while
   dragging.
6. **Circle** (ring) — drag a bounding box (start cell → current cell); on release, stamp the
   **outline** of the ellipse inscribed in that box with the brush tile (midpoint-ellipse,
   cell-granular). A preview is shown while dragging. Outline only (no filled mode).

### Erase model

There is no right-click. Erase a cell by drawing the blank tile 0 (select sprite 0 as the
brush), or `Select` a region and press `Delete`. This matches PICO-8.

### Interaction state machine

`tick` drives a small state machine so drags are unambiguous:

```
Idle
 ├─ Draw:   left held over map        → Painting (paint each frame at hovered cell)
 ├─ Paste:  left_pressed over map     → stamp clipboard once
 ├─ Select: left_pressed inside sel   → Moving { grabbed tiles, grab_cell }
 │          left_pressed elsewhere    → Selecting { anchor_cell } → drag updates rect
 ├─ Pan:    left_pressed over map     → Panning { anchor_mouse, anchor_cam }
 ├─ Fill:   left_pressed over map     → FillDrag { anchor_cell } → on release: flood (no drag) or rect
 └─ Circle: left_pressed over map     → ShapeDrag { anchor_cell } → on release: ellipse outline
```

State returns to `Idle` on button release. Toolbar/strip/page-dot clicks are handled first and
do not start a map drag.

## Clipboard & selection state

```
struct Selection { x: i32, y: i32, w: i32, h: i32 }   // map-space cells; scrolls with camera
struct Clipboard { w: i32, h: i32, tiles: Vec<u8> }    // row-major
```

Both are `Option` on `MapEditor`. `Selection` is normalised (positive w/h) when a drag ends.
Copy/cut read `assets.map`, paste/move write it via `MapData::set`, skipping out-of-bounds
cells. The console crate is `std`, so `Vec` is available (the sprite editor already uses `vec!`).

## Marching-ants animation

`MapEditor` gains `frame: u32`, incremented once per `tick` (the shell calls `tick` every
frame). `draw` is `&self` and reads it — no signature change. The selection border is drawn as a
1-px dashed outline whose dash phase advances with `frame`: walking the border pixels in order
with index `i`, the colour is white or black by `((i + frame/4) / DASH) % 2`, `DASH = 2`. This
gives a ~7 Hz march at 60 fps and reads clearly against the map. The border is clipped to the
map view (a selection may be partly scrolled off-screen).

## Hover highlight & status bar

When the cursor is over the map view, a 1-px white box outlines the hovered cell. The status bar
shows the hovered cell, the tile under it, the brush, and the page; during a selection drag it
shows the selection size instead:

```
normal:     "x{cx:03} y{cy:03} t{tile:03} b{brush:03} pg{page}"
selecting:  "x{cx:03} y{cy:03} sel {w}x{h}        b{brush:03} pg{page}"
```

`cx/cy` are the hovered map cell; `tile` is `assets.map.get(cx, cy)`. (~23 chars; the status bar
fits ~30 at `GLYPH_W=4`.)

## Shared-icon refactor

Both editors draw 8×8 1-bit tool icons. Move the helper and type from `editor/sprite.rs` into
`rico8-console/src/ui.rs` (made `pub`), and upgrade the pencil to the faithful PICO-8 glyph so
both editors share it:

```rust
pub type Icon8 = [u8; 8];
pub fn draw_icon8(fb: &mut Framebuffer, icon: &Icon8, x: i32, y: i32, color: u8);
pub const ICON_PENCIL: Icon8 = [0x08, 0x1C, 0x3E, 0x7C, 0xB8, 0x90, 0xE0, 0x00];
```

`sprite.rs` drops its local `Icon8`/`draw_icon8`, uses `ui::ICON_PENCIL` and `ui::draw_icon8`,
and keeps its eraser/fill/picker icons (unchanged behaviour). The map editor's tool icons
(sampled from PICO-8, glyph at columns 0–6, centred per-slot at draw time) live in `map.rs`:

```rust
const ICON_STAMP:  Icon8 = [0x38, 0x38, 0x38, 0x38, 0xFE, 0x82, 0xFE, 0x00]; // paste
const ICON_SELECT: Icon8 = [0xAA, 0x00, 0x82, 0x00, 0x82, 0x00, 0xAA, 0x00];
const ICON_HAND:   Icon8 = [0x28, 0x2A, 0x2A, 0x3E, 0xBE, 0x7E, 0x1C, 0x00]; // pan
const ICON_FILL:   Icon8 = [0x08, 0x04, 0x02, 0x7F, 0xBE, 0x9C, 0x88, 0x00]; // bucket
const ICON_CIRCLE: Icon8 = [0x38, 0x44, 0x82, 0x82, 0x82, 0x44, 0x38, 0x00];
```

Draw uses `ui::ICON_PENCIL` for Draw and these five for the rest.

## Keyboard

- Arrows: scroll the camera (clamped).
- PageUp/PageDown: change sheet page (mod 4).
- Tool letters: `d` Draw, `t` Paste (sTamp), `s` Select, `h` Pan, `f` Fill, `c` Circle
  (adjustable; chosen to avoid clashes — `Ctrl+C` etc. are gated on `mods.ctrl`).
- `Ctrl+C` / `Ctrl+X` / `Delete` operate on the active selection as above. (`Ctrl+V` is
  unnecessary — pasting is the Paste tool; a key can be added later if wanted.)

## Testing

The whole console is headless-testable (tests drive the same `Framebuffer` with no window).
Add `#[cfg(test)]` tests in `map.rs`, keeping logic in small testable methods (no `test_`
prefix per repo convention):

- Clicking each tool slot selects that `Tool`; clicking a page dot sets the page.
- `Select` over a region then `Ctrl+C` fills the clipboard with the expected tiles; `Paste` at a
  new cell reproduces them; out-of-bounds paste cells are skipped.
- `Ctrl+X` clears the source; `Delete` clears without touching the clipboard; move relocates the
  block (source cleared, destination written).
- Flood fill replaces only the contiguous same-tile region; rect fill covers the dragged box;
  circle stamps an outline within the box.
- The status-bar string matches the documented format for a known hover/selection.

Run with `SDL_VIDEODRIVER=dummy SDL_AUDIODRIVER=dummy cargo test -p rico8-console`. Match CI:
`cargo +nightly fmt --all`, `cargo clippy --workspace --all-targets -- -D warnings`.

## Dropped / deferred

- The separate eyedropper/pick tool is dropped — `Select`+copy/paste covers reusing tiles. A
  one-key "grab hovered tile as brush" can be added later as a keyboard shortcut (no button).
- No filled-circle mode (outline only), no numbered page tabs, no `Ctrl+V`. Easy follow-ups if
  desired.
