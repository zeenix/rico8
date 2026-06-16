# Map Editor Rework Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development
> (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use
> checkbox (`- [ ]`) syntax for tracking.

**Goal:** Rework the RICO-8 map editor into a PICO-8-style tool with six tools (draw, paste,
select, pan, fill, circle), a clipboard, an animated marching-ants selection, a hover highlight,
a richer status bar, and a full-page sprite sheet with page dots.

**Architecture:** All rendering is software into the shared `Framebuffer`; the editor owns
y 8–119 plus the status bar. The shell already calls `key(key, mods, assets)` and
`tick(&mouse, assets)` every frame and `draw(&self, fb, assets)`, so a per-frame `frame`
counter (incremented in `tick`) drives the marching-ants animation with no signature change.
The 8×8 tool-icon helper is hoisted into the shared `ui` module so both the map and sprite
editors share it (and the upgraded pencil glyph).

**Tech Stack:** Rust, `rico8-console` (winit/wgpu frontend, but headless-testable), `rico8-runtime`
assets/framebuffer. Tests run with `SDL_VIDEODRIVER=dummy SDL_AUDIODRIVER=dummy`.

**Conventions:** Commit messages use a verbatim [gimoji](https://gitmoji.dev/) emoji prefix —
run `gimoji` (or copy from <https://zeenix.github.io/gimoji/>) rather than typing from memory, so
the U+FE0F variation selectors are exact (CI lints this). Suggested prefixes per task are below.
Format with `cargo +nightly fmt --all` before each commit; clippy must pass with `-D warnings`.

**Spec:** `docs/superpowers/specs/2026-06-16-map-editor-rework-design.md`

---

## File Structure

- **Modify** `rico8-console/src/ui.rs` — add the shared `Icon8` type, `draw_icon8`, and the
  upgraded `ICON_PENCIL` (sampled from PICO-8). New unit test in the existing `tests` module.
- **Modify** `rico8-console/src/editor/sprite.rs` — drop the local `Icon8`/`draw_icon8`/
  `ICON_PENCIL`; use `ui::Icon8`, `ui::draw_icon8`, `ui::ICON_PENCIL`. No behaviour change.
- **Rewrite** `rico8-console/src/editor/map.rs` — the bulk of the work, built up across Tasks
  2–8 with a `#[cfg(test)] mod tests`.

No changes to `rico8-runtime`, the ABI, the asset model, or the shell dispatch.

---

## Task 1: Shared icon helper + pencil upgrade

**Files:**
- Modify: `rico8-console/src/ui.rs`
- Modify: `rico8-console/src/editor/sprite.rs`

- [ ] **Step 1: Write the failing test**

Add to the existing `#[cfg(test)] mod tests` in `rico8-console/src/ui.rs`:

```rust
#[test]
fn pencil_icon_draws_its_lit_pixels() {
    let mut fb = Framebuffer::new();
    draw_icon8(&mut fb, &ICON_PENCIL, 10, 20, col::WHITE);
    // Row 0 of ICON_PENCIL (0x08) lights exactly column 4.
    assert_eq!(fb.pget(10 + 4, 20), col::WHITE, "row 0 bit 4 lit");
    assert_eq!(fb.pget(10 + 0, 20), col::BLACK, "row 0 bit 0 unlit");
    // Row 6 (0xE0) lights columns 0..=2.
    assert_eq!(fb.pget(10 + 0, 26), col::WHITE, "row 6 bit 0 lit");
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `SDL_VIDEODRIVER=dummy SDL_AUDIODRIVER=dummy cargo test -p rico8-console pencil_icon_draws -- --nocapture`
Expected: FAIL — `draw_icon8` and `ICON_PENCIL` are not defined in `ui`.

- [ ] **Step 3: Add the shared helper to `ui.rs`**

Add near the top of `rico8-console/src/ui.rs` (after the `use` block):

```rust
/// An 8×8 one-bit icon: each byte is a row, MSB is the left pixel.
pub type Icon8 = [u8; 8];

/// Blit an [`Icon8`] at (x, y) in the given colour; unset bits are left untouched.
pub fn draw_icon8(fb: &mut Framebuffer, icon: &Icon8, x: i32, y: i32, color: u8) {
    for (ry, row) in icon.iter().enumerate() {
        for rx in 0..8 {
            if row & (0x80 >> rx) != 0 {
                fb.pset(x + rx, y + ry as i32, color);
            }
        }
    }
}

/// The pencil glyph, sampled pixel-for-pixel from PICO-8 (shared by the sprite
/// and map editors).
pub const ICON_PENCIL: Icon8 = [0x08, 0x1C, 0x3E, 0x7C, 0xB8, 0x90, 0xE0, 0x00];
```

- [ ] **Step 4: Run test to verify it passes**

Run: `SDL_VIDEODRIVER=dummy SDL_AUDIODRIVER=dummy cargo test -p rico8-console pencil_icon_draws`
Expected: PASS.

- [ ] **Step 5: Point the sprite editor at the shared helper**

In `rico8-console/src/editor/sprite.rs`:

1. Change the `use crate::...` block to import the helper:
   ```rust
   use crate::{
       shell::{Key, Mods},
       ui::{self, draw_icon8, Icon8, Mouse, ICON_PENCIL},
   };
   ```
2. Delete the local `type Icon8 = [u8; 8];`, the local `const ICON_PENCIL: Icon8 = [...]`, and the
   local `fn draw_icon8(...)` (the four lines plus body near the bottom of the file).
3. Keep `ICON_ERASER`, `ICON_FILL`, `ICON_PICKER` as they are (they now reference `ui::Icon8` via
   the import). The call sites `draw_icon8(fb, icon, x, 9, color)` and the tools array entry
   `(Tool::Pencil, ICON_PENCIL)` now resolve to the shared items unchanged.

- [ ] **Step 6: Verify the workspace builds, lints, and the sprite editor still works**

Run:
```
cargo +nightly fmt --all
SDL_VIDEODRIVER=dummy SDL_AUDIODRIVER=dummy cargo test -p rico8-console
cargo clippy --workspace --all-targets -- -D warnings
```
Expected: all PASS, no warnings.

- [ ] **Step 7: Commit**

```bash
git add rico8-console/src/ui.rs rico8-console/src/editor/sprite.rs
git commit -m "$(gimoji -s '♻️ Share the 8x8 tool-icon helper and PICO-8 pencil glyph')"
```
(Use `gimoji` so the emoji/variation-selector is exact; the message is "Share the 8x8 tool-icon
helper and PICO-8 pencil glyph".)

---

## Task 2: Map editor foundation — layout, tools, draw, selection of tool/page/brush

This task replaces `rico8-console/src/editor/map.rs` wholesale with the new scaffold: the struct,
layout constants, `Tool` enum, icon constants, the full `draw`, and the parts of `tick`/`key`
that pick a tool, page, and brush. Per-tool map interactions land in Tasks 3–8.

**Files:**
- Rewrite: `rico8-console/src/editor/map.rs`

- [ ] **Step 1: Write the failing tests**

Replace any existing `mod tests` in `map.rs` with this module at the end of the file:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use rico8_runtime::assets::Assets;

    fn press(x: i32, y: i32) -> Mouse {
        Mouse { x, y, left: true, left_pressed: true, ..Default::default() }
    }

    #[test]
    fn clicking_a_tool_slot_selects_it() {
        let mut ed = MapEditor::new();
        let mut a = Assets::default();
        // Slot 2 is Select (0=Draw,1=Paste,2=Select).
        ed.tick(&press(tool_x(2) + 1, 10), &mut a);
        assert_eq!(ed.tool, Tool::Select);
    }

    #[test]
    fn clicking_a_page_dot_sets_the_page() {
        let mut ed = MapEditor::new();
        let mut a = Assets::default();
        ed.tick(&press(PAGE_X + 2 * 6 + 1, PAGE_Y + 1), &mut a);
        assert_eq!(ed.page, 2);
    }

    #[test]
    fn clicking_the_sheet_sets_the_brush() {
        let mut ed = MapEditor::new();
        let mut a = Assets::default();
        ed.page = 1; // page 1 starts at sprite 64
        // Second row, third column of the strip -> 64 + 1*16 + 2 = 82.
        ed.tick(&press(2 * 8 + 1, SHEET_Y + 8 + 1), &mut a);
        assert_eq!(ed.brush, 82);
    }
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `SDL_VIDEODRIVER=dummy SDL_AUDIODRIVER=dummy cargo test -p rico8-console -- map`
Expected: FAIL to compile — `MapEditor`, `Tool`, `tool_x`, `PAGE_X`, etc. not yet defined as
shown.

- [ ] **Step 3: Write the new `map.rs` scaffold**

Replace the entire contents of `rico8-console/src/editor/map.rs` with:

```rust
//! The map editor: a scrollable tile viewport with six PICO-8-style tools
//! (draw, paste, select, pan, fill, circle), a clipboard, and a full-page
//! sprite sheet picker along the bottom.

use crate::{
    shell::{Key, Mods},
    ui::{self, draw_icon8, Icon8, Mouse, ICON_PENCIL},
};
use rico8_runtime::{
    assets::{Assets, MAP_H, MAP_W, SPRITES_PER_ROW},
    fb::Framebuffer,
    palette::col,
};

// --- Layout (see the design spec) -------------------------------------------
const TOOLBAR_Y: i32 = 8; // tool icons drawn at y 9
const VIEW_Y: i32 = 18;
const VIEW_TILES_X: i32 = 16;
const VIEW_TILES_Y: i32 = 8;
const VIEW_BOTTOM: i32 = VIEW_Y + VIEW_TILES_Y * 8 - 1; // 81
const SEP_Y: i32 = 82;
const SHEET_Y: i32 = 84; // 4 rows of 8 px
const SHEET_BOTTOM: i32 = SHEET_Y + 4 * 8 - 1; // 115
const BRUSH_X: i32 = 88; // 8×8 brush preview
const PAGE_X: i32 = 104; // four 5×5 page dots
const PAGE_Y: i32 = 11;

fn tool_x(i: usize) -> i32 {
    2 + i as i32 * 9
}

// --- Tool icons (Draw reuses the shared pencil; rest sampled from PICO-8) ----
const ICON_STAMP: Icon8 = [0x38, 0x38, 0x38, 0x38, 0xFE, 0x82, 0xFE, 0x00]; // paste
const ICON_SELECT: Icon8 = [0xAA, 0x00, 0x82, 0x00, 0x82, 0x00, 0xAA, 0x00];
const ICON_HAND: Icon8 = [0x28, 0x2A, 0x2A, 0x3E, 0xBE, 0x7E, 0x1C, 0x00]; // pan
const ICON_FILL: Icon8 = [0x08, 0x04, 0x02, 0x7F, 0xBE, 0x9C, 0x88, 0x00]; // bucket
const ICON_CIRCLE: Icon8 = [0x38, 0x44, 0x82, 0x82, 0x82, 0x44, 0x38, 0x00];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Tool {
    Draw,
    Paste,
    Select,
    Pan,
    Fill,
    Circle,
}

// Icons stored by value (`Icon8` is `[u8; 8]`, `Copy`), matching the sprite
// editor's `tools` array, so iterating yields `icon: &Icon8` for `draw_icon8`.
const TOOLS: [(Tool, Icon8); 6] = [
    (Tool::Draw, ICON_PENCIL),
    (Tool::Paste, ICON_STAMP),
    (Tool::Select, ICON_SELECT),
    (Tool::Pan, ICON_HAND),
    (Tool::Fill, ICON_FILL),
    (Tool::Circle, ICON_CIRCLE),
];

/// A rectangular cell selection, in map space (it scrolls with the camera).
#[derive(Debug, Clone, Copy)]
struct Selection {
    x: i32,
    y: i32,
    w: i32,
    h: i32,
}

/// A copied block of tiles, row-major.
#[derive(Debug, Clone)]
struct Clipboard {
    w: i32,
    h: i32,
    tiles: Vec<u8>,
}

/// The in-progress mouse drag, if any.
#[derive(Debug, Clone)]
enum Drag {
    None,
    Selecting { ax: i32, ay: i32 },
    Moving { tiles: Vec<u8>, w: i32, h: i32, gx: i32, gy: i32 },
    Panning { amx: i32, amy: i32, acx: i32, acy: i32 },
    Shape { ax: i32, ay: i32 }, // fill-rect and circle previews
}

pub struct MapEditor {
    tool: Tool,
    brush: u32,
    cam_x: i32,
    cam_y: i32,
    page: u32,
    frame: u32,
    /// Last-known cursor cell over the view (for hover box / previews).
    mx: i32,
    my: i32,
    sel: Option<Selection>,
    clip: Option<Clipboard>,
    drag: Drag,
}

impl MapEditor {
    pub fn new() -> Self {
        Self {
            tool: Tool::Draw,
            brush: 1,
            cam_x: 0,
            cam_y: 0,
            page: 0,
            frame: 0,
            mx: -16,
            my: -16,
            sel: None,
            clip: None,
            drag: Drag::None,
        }
    }

    pub fn key(&mut self, key: Key, _mods: Mods, _assets: &mut Assets) {
        match key {
            Key::Left => self.cam_x = (self.cam_x - 1).max(0),
            Key::Right => self.cam_x = (self.cam_x + 1).min(MAP_W as i32 - VIEW_TILES_X),
            Key::Up => self.cam_y = (self.cam_y - 1).max(0),
            Key::Down => self.cam_y = (self.cam_y + 1).min(MAP_H as i32 - VIEW_TILES_Y),
            Key::PageUp => self.page = (self.page + 3) % 4,
            Key::PageDown => self.page = (self.page + 1) % 4,
            Key::Char('d') => self.tool = Tool::Draw,
            Key::Char('t') => self.tool = Tool::Paste,
            Key::Char('s') => self.tool = Tool::Select,
            Key::Char('h') => self.tool = Tool::Pan,
            Key::Char('f') => self.tool = Tool::Fill,
            Key::Char('c') => self.tool = Tool::Circle,
            _ => {}
        }
    }

    // `_assets` is unused until Task 3 adds the per-tool match; the signature
    // must stay `&mut Assets` for the shell's `map_ed.tick(&mouse, a)` call.
    pub fn tick(&mut self, mouse: &Mouse, _assets: &mut Assets) {
        self.frame = self.frame.wrapping_add(1);
        self.mx = mouse.x;
        self.my = mouse.y;

        // Toolbar / sheet / page-dot clicks take priority over map drags.
        if mouse.left_pressed && self.handle_chrome_click(mouse) {
            return;
        }
        // Per-tool map interaction is added in later tasks.
    }

    /// Handle a click on the toolbar, page dots, or sprite sheet. Returns true
    /// if the click was consumed.
    fn handle_chrome_click(&mut self, m: &Mouse) -> bool {
        // Tool icons.
        for (i, (tool, _)) in TOOLS.iter().enumerate() {
            let x = tool_x(i);
            if m.over(x, TOOLBAR_Y + 1, x + 7, TOOLBAR_Y + 8) {
                self.tool = *tool;
                return true;
            }
        }
        // Page dots.
        for p in 0..4 {
            let x = PAGE_X + p * 6;
            if m.over(x, PAGE_Y, x + 4, PAGE_Y + 4) {
                self.page = p as u32;
                return true;
            }
        }
        // Sprite sheet: pick the brush.
        if m.over(0, SHEET_Y, 127, SHEET_BOTTOM) {
            let cx = m.x / 8;
            let cy = (m.y - SHEET_Y) / 8;
            self.brush = (self.page * 64 + cy as u32 * 16 + cx as u32) % 256;
            return true;
        }
        false
    }

    pub fn draw(&self, fb: &mut Framebuffer, assets: &Assets) {
        self.draw_toolbar(fb, assets);
        self.draw_view(fb, assets);
        fb.rectfill(0, SEP_Y, 127, SEP_Y + 1, col::LIGHT_GREY);
        self.draw_sheet(fb, assets);
        self.draw_status(fb, assets);
    }

    fn draw_toolbar(&self, fb: &mut Framebuffer, assets: &Assets) {
        fb.rectfill(0, TOOLBAR_Y, 127, VIEW_Y - 1, col::DARK_GREY);
        for (i, (tool, icon)) in TOOLS.iter().enumerate() {
            let x = tool_x(i);
            let color = if *tool == self.tool {
                fb.rectfill(x - 1, TOOLBAR_Y, x + 8, TOOLBAR_Y + 8, col::BLACK);
                col::WHITE
            } else {
                col::DARK_PURPLE
            };
            draw_icon8(fb, icon, x, TOOLBAR_Y + 1, color);
        }
        // Brush preview (the selected sprite, drawn as-is).
        let (ox, oy) = sprite_origin(self.brush);
        for py in 0..8 {
            for px in 0..8 {
                fb.pset(BRUSH_X + px, TOOLBAR_Y + 1 + py, assets.sprites.get(ox + px, oy + py));
            }
        }
        fb.rect(BRUSH_X - 1, TOOLBAR_Y, BRUSH_X + 8, TOOLBAR_Y + 9, col::BLACK);
        // Page dots.
        for p in 0..4u32 {
            let x = PAGE_X + p as i32 * 6;
            let c = if p == self.page { col::WHITE } else { col::DARK_PURPLE };
            fb.rectfill(x, PAGE_Y, x + 4, PAGE_Y + 4, c);
        }
    }

    fn draw_view(&self, fb: &mut Framebuffer, assets: &Assets) {
        fb.rectfill(0, VIEW_Y, 127, VIEW_BOTTOM, col::BLACK);
        fb.map(
            &assets.map,
            &assets.sprites,
            self.cam_x,
            self.cam_y,
            0,
            VIEW_Y,
            VIEW_TILES_X,
            VIEW_TILES_Y,
            0,
        );
    }

    fn draw_sheet(&self, fb: &mut Framebuffer, assets: &Assets) {
        fb.rectfill(0, SHEET_Y, 127, SHEET_BOTTOM, col::DARK_GREY);
        for cy in 0..4i32 {
            for cx in 0..16i32 {
                let n = self.page * 64 + (cy * 16 + cx) as u32;
                let (sx, sy) = sprite_origin(n);
                for py in 0..8 {
                    for px in 0..8 {
                        fb.pset(cx * 8 + px, SHEET_Y + cy * 8 + py, assets.sprites.get(sx + px, sy + py));
                    }
                }
            }
        }
        if self.brush / 64 == self.page {
            let i = self.brush % 64;
            let x = (i as i32 % 16) * 8;
            let y = SHEET_Y + (i as i32 / 16) * 8;
            fb.rect(x, y, x + 7, y + 7, col::WHITE);
        }
    }

    fn draw_status(&self, fb: &mut Framebuffer, assets: &Assets) {
        let text = match self.hovered_cell() {
            Some((cx, cy)) => format!(
                "x{:03} y{:03} t{:03} b{:03} pg{}",
                cx,
                cy,
                assets.map.get(cx, cy),
                self.brush,
                self.page
            ),
            None => format!("b{:03} pg{}", self.brush, self.page),
        };
        ui::status_bar(fb, &text);
    }

    /// The map cell under the cursor, if the cursor is over the view.
    fn hovered_cell(&self) -> Option<(i32, i32)> {
        if self.mx < 0 || self.mx > 127 || self.my < VIEW_Y || self.my > VIEW_BOTTOM {
            return None;
        }
        Some((self.cam_x + self.mx / 8, self.cam_y + (self.my - VIEW_Y) / 8))
    }
}

fn sprite_origin(n: u32) -> (i32, i32) {
    (
        (n as i32 % SPRITES_PER_ROW as i32) * 8,
        (n as i32 / SPRITES_PER_ROW as i32) * 8,
    )
}
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `SDL_VIDEODRIVER=dummy SDL_AUDIODRIVER=dummy cargo test -p rico8-console -- map`
Expected: PASS (3 tests).

**Do NOT run `clippy -D warnings` for Tasks 2–7.** `frame`, `sel`, `clip`, `drag`, the `Drag`
variants, `Selection`, and `Clipboard` are constructed/read only by Tasks 4–8, so until then they
produce dead-code *warnings* — which is fine: `cargo build` and `cargo test` succeed with warnings,
and **no `#[allow(dead_code)]` is ever added**. The `clippy -D warnings` gate runs once, in Task 8,
after every item is used. Optionally `cargo build -p rico8-console` here to confirm it compiles.

- [ ] **Step 5: Format and commit**

```bash
cargo +nightly fmt --all
git add rico8-console/src/editor/map.rs
git commit -m "$(gimoji -s '💄 Rework the map editor layout, toolbar, and full-page sheet')"
```

---

## Task 3: Draw tool — painting + hover highlight + status hover info

**Files:**
- Modify: `rico8-console/src/editor/map.rs`

- [ ] **Step 1: Write the failing tests**

Add to `mod tests`:

```rust
#[test]
fn draw_tool_paints_the_hovered_cell() {
    let mut ed = MapEditor::new();
    let mut a = Assets::default();
    ed.brush = 7;
    // Hover cell (1, 0): x in [8,15], y in [VIEW_Y, VIEW_Y+7].
    ed.tick(&press(9, VIEW_Y + 1), &mut a);
    assert_eq!(a.map.get(1, 0), 7);
}

#[test]
fn draw_respects_the_camera_offset() {
    let mut ed = MapEditor::new();
    let mut a = Assets::default();
    ed.brush = 5;
    ed.cam_x = 10;
    ed.cam_y = 4;
    let held = Mouse { x: 0, y: VIEW_Y, left: true, ..Default::default() };
    ed.tick(&held, &mut a);
    assert_eq!(a.map.get(10, 4), 5);
}

#[test]
fn hover_box_outlines_the_cell_under_the_cursor() {
    let mut ed = MapEditor::new();
    let mut a = Assets::default();
    let hover = Mouse { x: 9, y: VIEW_Y + 1, ..Default::default() };
    ed.tick(&hover, &mut a);
    let mut fb = Framebuffer::new();
    ed.draw(&mut fb, &a);
    // Cell (1,0) screen rect top-left corner at (8, VIEW_Y).
    assert_eq!(fb.pget(8, VIEW_Y), col::WHITE);
}
```

- [ ] **Step 2: Run to verify they fail**

Run: `SDL_VIDEODRIVER=dummy SDL_AUDIODRIVER=dummy cargo test -p rico8-console -- map`
Expected: `draw_tool_paints_the_hovered_cell` and the others FAIL (no painting / no hover box).

- [ ] **Step 3: Add painting to `tick` and the hover box to `draw`**

First rename the `tick` parameter `_assets` back to `assets` (it is now used). Then, after the
`handle_chrome_click` early-return, append the per-tool match (later tasks add arms before the
`_ => {}`):

```rust
        // Per-tool map interaction.
        match self.tool {
            Tool::Draw => {
                if mouse.left {
                    if let Some((cx, cy)) = self.hovered_cell() {
                        assets.map.set(cx, cy, self.brush as u8);
                    }
                }
            }
            _ => {}
        }
```

In `draw`, change the `draw_view` call site so the hover box is drawn after the map. Add to the
end of `draw_view`:

```rust
        if let Some((cx, cy)) = self.hovered_cell() {
            let sx = (cx - self.cam_x) * 8;
            let sy = VIEW_Y + (cy - self.cam_y) * 8;
            fb.rect(sx, sy, sx + 7, sy + 7, col::WHITE);
        }
```

- [ ] **Step 4: Run to verify they pass**

Run: `SDL_VIDEODRIVER=dummy SDL_AUDIODRIVER=dummy cargo test -p rico8-console -- map`
Expected: PASS.

- [ ] **Step 5: Format and commit**

```bash
cargo +nightly fmt --all
git add rico8-console/src/editor/map.rs
git commit -m "$(gimoji -s '✨ Add the map draw tool, hover highlight, and cell status info')"
```

---

## Task 4: Select tool — marquee + animated marching ants

**Files:**
- Modify: `rico8-console/src/editor/map.rs`

- [ ] **Step 1: Write the failing tests**

Add to `mod tests`:

```rust
// A held drag frame (button down, no fresh press edge).
fn held(x: i32, y: i32) -> Mouse {
    Mouse { x, y, left: true, ..Default::default() }
}
// A release frame: the real input keeps the cursor position and only drops
// `left` (the shell updates x/y on CursorMoved, not MouseInput), so release at
// the drag-end coordinates — NOT off-screen.
fn rel(x: i32, y: i32) -> Mouse {
    Mouse { x, y, ..Default::default() }
}

#[test]
fn select_drag_builds_a_normalized_selection() {
    let mut ed = MapEditor::new();
    let mut a = Assets::default();
    ed.tool = Tool::Select;
    // Press at cell (3,1), drag up-left to cell (1,0), release there.
    ed.tick(&press(3 * 8 + 1, VIEW_Y + 1 * 8 + 1), &mut a);
    ed.tick(&held(1 * 8 + 1, VIEW_Y + 1), &mut a);
    ed.tick(&rel(1 * 8 + 1, VIEW_Y + 1), &mut a);
    let s = ed.sel.expect("selection set");
    assert_eq!((s.x, s.y, s.w, s.h), (1, 0, 3, 2));
}

#[test]
fn selecting_shows_size_in_status_bar() {
    let mut ed = MapEditor::new();
    let mut a = Assets::default();
    ed.tool = Tool::Select;
    ed.tick(&press(8 + 1, VIEW_Y + 1), &mut a); // cell (1,0)
    ed.tick(&held(3 * 8 + 1, VIEW_Y + 8 + 1), &mut a); // drag to (3,1)
    assert!(matches!(ed.drag, Drag::Selecting { .. }));
    assert_eq!(ed.selection_in_progress(), Some((1, 0, 3, 2)));
}
```

- [ ] **Step 2: Run to verify they fail**

Run: `SDL_VIDEODRIVER=dummy SDL_AUDIODRIVER=dummy cargo test -p rico8-console -- map`
Expected: FAIL — `selection_in_progress` undefined and no select handling.

- [ ] **Step 3: Implement select-drag, normalization, and marching ants**

Add a helper near `hovered_cell` (clamps the cursor to the view so off-edge drags still resolve a
cell):

```rust
    /// The cursor's map cell, clamped to the visible view (for drags).
    fn clamped_cell(&self) -> (i32, i32) {
        let mx = self.mx.clamp(0, 127);
        let my = self.my.clamp(VIEW_Y, VIEW_BOTTOM);
        (self.cam_x + mx / 8, self.cam_y + (my - VIEW_Y) / 8)
    }

    /// The rectangle (x, y, w, h) of an in-progress selection drag, if any.
    fn selection_in_progress(&self) -> Option<(i32, i32, i32, i32)> {
        if let Drag::Selecting { ax, ay } = self.drag {
            let (cx, cy) = self.clamped_cell();
            Some(normalize_rect(ax, ay, cx, cy))
        } else {
            None
        }
    }
```

Add the free function at the bottom of the file:

```rust
fn normalize_rect(ax: i32, ay: i32, bx: i32, by: i32) -> (i32, i32, i32, i32) {
    let x = ax.min(bx);
    let y = ay.min(by);
    (x, y, (ax - bx).abs() + 1, (ay - by).abs() + 1)
}
```

Replace the `match self.tool { ... }` block in `tick` with the version below (Draw retained,
Select added). Note the press/drag/release handling sits **outside** the priority chrome check,
so it runs whenever a tool is active:

```rust
        match self.tool {
            Tool::Draw => {
                if mouse.left {
                    if let Some((cx, cy)) = self.hovered_cell() {
                        assets.map.set(cx, cy, self.brush as u8);
                    }
                }
            }
            Tool::Select => {
                if mouse.left_pressed && mouse.y >= VIEW_Y && mouse.y <= VIEW_BOTTOM {
                    let (cx, cy) = self.clamped_cell();
                    self.drag = Drag::Selecting { ax: cx, ay: cy };
                }
                if !mouse.left {
                    if let Drag::Selecting { .. } = self.drag {
                        let (x, y, w, h) = self.selection_in_progress().unwrap();
                        self.sel = Some(Selection { x, y, w, h });
                        self.drag = Drag::None;
                    }
                }
            }
            _ => {}
        }
```

Add marching-ants rendering. Append to `draw` after `draw_view`'s call (i.e. in `draw`, before
the separator), a call `self.draw_selection(fb);` and add the method:

```rust
    fn draw_selection(&self, fb: &mut Framebuffer) {
        let rect = self
            .selection_in_progress()
            .or_else(|| self.sel.map(|s| (s.x, s.y, s.w, s.h)));
        let Some((x, y, w, h)) = rect else { return };
        let sx = (x - self.cam_x) * 8;
        let sy = VIEW_Y + (y - self.cam_y) * 8;
        let (x0, y0, x1, y1) = (sx, sy, sx + w * 8 - 1, sy + h * 8 - 1);
        let mut i = 0i32;
        let ant = |fb: &mut Framebuffer, px: i32, py: i32, i: &mut i32| {
            if (0..=127).contains(&px) && (VIEW_Y..=VIEW_BOTTOM).contains(&py) {
                let on = ((*i + (self.frame / 4) as i32) / 2) % 2 == 0;
                fb.pset(px, py, if on { col::WHITE } else { col::BLACK });
            }
            *i += 1;
        };
        for px in x0..=x1 {
            ant(fb, px, y0, &mut i);
        }
        for py in (y0 + 1)..=y1 {
            ant(fb, x1, py, &mut i);
        }
        for px in (x0..x1).rev() {
            ant(fb, px, y1, &mut i);
        }
        for py in ((y0 + 1)..y1).rev() {
            ant(fb, x0, py, &mut i);
        }
    }
```

In `draw`, place `self.draw_selection(fb);` between `self.draw_view(...)` and the separator
`rectfill`. Remove the dead-code allow on `Selection` (if you added one in Task 2).

- [ ] **Step 4: Run to verify they pass**

Run: `SDL_VIDEODRIVER=dummy SDL_AUDIODRIVER=dummy cargo test -p rico8-console -- map`
Expected: PASS.

- [ ] **Step 5: Update the status bar to show the selection size**

In `draw_status`, prefer the in-progress selection size when present:

```rust
        let text = if let Some((_, _, w, h)) = self.selection_in_progress() {
            format!("sel {}x{}        b{:03} pg{}", w, h, self.brush, self.page)
        } else {
            match self.hovered_cell() {
                Some((cx, cy)) => format!(
                    "x{:03} y{:03} t{:03} b{:03} pg{}",
                    cx, cy, assets.map.get(cx, cy), self.brush, self.page
                ),
                None => format!("b{:03} pg{}", self.brush, self.page),
            }
        };
```

- [ ] **Step 6: Run, format, commit**

```bash
SDL_VIDEODRIVER=dummy SDL_AUDIODRIVER=dummy cargo test -p rico8-console -- map
cargo +nightly fmt --all
git add rico8-console/src/editor/map.rs
git commit -m "$(gimoji -s '✨ Add the map select tool with an animated marching-ants border')"
```

---

## Task 5: Clipboard — copy/cut/delete, paste tool, and move

**Files:**
- Modify: `rico8-console/src/editor/map.rs`

- [ ] **Step 1: Write the failing tests**

Add to `mod tests`:

```rust
fn make_selection(ed: &mut MapEditor, a: &mut Assets, x: i32, y: i32, w: i32, h: i32) {
    ed.tool = Tool::Select;
    ed.tick(&press(x * 8 + 1, VIEW_Y + y * 8 + 1), a);
    ed.tick(&held((x + w - 1) * 8 + 1, VIEW_Y + (y + h - 1) * 8 + 1), a);
    ed.tick(&rel((x + w - 1) * 8 + 1, VIEW_Y + (y + h - 1) * 8 + 1), a);
}

#[test]
fn copy_then_paste_reproduces_the_block() {
    let mut ed = MapEditor::new();
    let mut a = Assets::default();
    a.map.set(2, 1, 11);
    a.map.set(3, 1, 12);
    make_selection(&mut ed, &mut a, 2, 1, 2, 1);
    ed.key(Key::Char('c'), Mods { ctrl: true, ..Default::default() }, &mut a);
    let clip = ed.clip.clone().expect("clipboard filled");
    assert_eq!((clip.w, clip.h, clip.tiles), (2, 1, vec![11, 12]));
    // Paste at cell (5, 4).
    ed.tool = Tool::Paste;
    ed.tick(&press(5 * 8 + 1, VIEW_Y + 4 * 8 + 1), &mut a);
    assert_eq!(a.map.get(5, 4), 11);
    assert_eq!(a.map.get(6, 4), 12);
}

#[test]
fn cut_clears_the_source() {
    let mut ed = MapEditor::new();
    let mut a = Assets::default();
    a.map.set(2, 1, 11);
    make_selection(&mut ed, &mut a, 2, 1, 1, 1);
    ed.key(Key::Char('x'), Mods { ctrl: true, ..Default::default() }, &mut a);
    assert_eq!(a.map.get(2, 1), 0);
    assert_eq!(ed.clip.as_ref().unwrap().tiles, vec![11]);
}

#[test]
fn delete_clears_without_touching_the_clipboard() {
    let mut ed = MapEditor::new();
    let mut a = Assets::default();
    a.map.set(2, 1, 11);
    make_selection(&mut ed, &mut a, 2, 1, 1, 1);
    ed.key(Key::Delete, Mods::default(), &mut a);
    assert_eq!(a.map.get(2, 1), 0);
    assert!(ed.clip.is_none());
}

#[test]
fn dragging_inside_a_selection_moves_the_block() {
    let mut ed = MapEditor::new();
    let mut a = Assets::default();
    a.map.set(2, 1, 11);
    make_selection(&mut ed, &mut a, 2, 1, 1, 1);
    // Press inside the selection (cell 2,1), drag to cell (5,4), release there.
    ed.tick(&press(2 * 8 + 1, VIEW_Y + 1 * 8 + 1), &mut a);
    ed.tick(&held(5 * 8 + 1, VIEW_Y + 4 * 8 + 1), &mut a);
    ed.tick(&rel(5 * 8 + 1, VIEW_Y + 4 * 8 + 1), &mut a);
    assert_eq!(a.map.get(2, 1), 0, "source cleared");
    assert_eq!(a.map.get(5, 4), 11, "block moved");
}
```

- [ ] **Step 2: Run to verify they fail**

Run: `SDL_VIDEODRIVER=dummy SDL_AUDIODRIVER=dummy cargo test -p rico8-console -- map`
Expected: FAIL — no clipboard/paste/move handling.

- [ ] **Step 3: Implement clipboard ops in `key`, paste + move in `tick`**

Add these methods to `impl MapEditor`. `copy_selection` takes `&mut Assets` and clears the
source inline when cutting (so there is no second borrow of `assets`):

```rust
    fn copy_selection(&mut self, assets: &mut Assets, cut: bool) {
        let Some(s) = self.sel else { return };
        let mut tiles = Vec::with_capacity((s.w * s.h) as usize);
        for y in 0..s.h {
            for x in 0..s.w {
                tiles.push(assets.map.get(s.x + x, s.y + y));
            }
        }
        self.clip = Some(Clipboard { w: s.w, h: s.h, tiles });
        if cut {
            for y in 0..s.h {
                for x in 0..s.w {
                    assets.map.set(s.x + x, s.y + y, 0);
                }
            }
        }
    }

    fn delete_selection(&mut self, assets: &mut Assets) {
        let Some(s) = self.sel else { return };
        for y in 0..s.h {
            for x in 0..s.w {
                assets.map.set(s.x + x, s.y + y, 0);
            }
        }
    }

    fn paste_at(&self, assets: &mut Assets, cx: i32, cy: i32) {
        let Some(clip) = &self.clip else { return };
        for y in 0..clip.h {
            for x in 0..clip.w {
                let (dx, dy) = (cx + x, cy + y);
                if (0..MAP_W as i32).contains(&dx) && (0..MAP_H as i32).contains(&dy) {
                    assets.map.set(dx, dy, clip.tiles[(y * clip.w + x) as usize]);
                }
            }
        }
    }

    fn point_in_selection(&self, cx: i32, cy: i32) -> bool {
        match self.sel {
            Some(s) => cx >= s.x && cx < s.x + s.w && cy >= s.y && cy < s.y + s.h,
            None => false,
        }
    }
```

Extend `key` to handle the clipboard shortcuts — replace its `match key` arms for `Char('c')`,
`Char('x')`, and `Delete`/`Backspace`. Because plain `c` is the Circle tool but `Ctrl+C` copies,
gate on `_mods` (rename the param to `mods`):

```rust
    pub fn key(&mut self, key: Key, mods: Mods, assets: &mut Assets) {
        match key {
            Key::Left => self.cam_x = (self.cam_x - 1).max(0),
            Key::Right => self.cam_x = (self.cam_x + 1).min(MAP_W as i32 - VIEW_TILES_X),
            Key::Up => self.cam_y = (self.cam_y - 1).max(0),
            Key::Down => self.cam_y = (self.cam_y + 1).min(MAP_H as i32 - VIEW_TILES_Y),
            Key::PageUp => self.page = (self.page + 3) % 4,
            Key::PageDown => self.page = (self.page + 1) % 4,
            Key::Char('c') if mods.ctrl => self.copy_selection(assets, false),
            Key::Char('x') if mods.ctrl => self.copy_selection(assets, true),
            Key::Delete | Key::Backspace => self.delete_selection(assets),
            Key::Char('d') => self.tool = Tool::Draw,
            Key::Char('t') => self.tool = Tool::Paste,
            Key::Char('s') => self.tool = Tool::Select,
            Key::Char('h') => self.tool = Tool::Pan,
            Key::Char('f') => self.tool = Tool::Fill,
            Key::Char('c') => self.tool = Tool::Circle,
            _ => {}
        }
    }
```

Add a `Tool::Paste` arm to the per-tool `match` in `tick`, and **replace** the `Tool::Select` arm
from Task 4 with the move-aware version below (it begins a move when the press lands inside the
current selection, otherwise starts a new marquee, and resolves either on release). The
`Drag::Moving` arm binds `tiles: Vec<u8>` (not `Copy`), so it takes the drag by value via
`std::mem::replace`; `Drag::Selecting` binds only `Copy` fields:

```rust
            Tool::Paste => {
                if mouse.left_pressed {
                    if let Some((cx, cy)) = self.hovered_cell() {
                        self.paste_at(assets, cx, cy);
                    }
                }
            }
            Tool::Select => {
                if mouse.left_pressed && mouse.y >= VIEW_Y && mouse.y <= VIEW_BOTTOM {
                    let (cx, cy) = self.clamped_cell();
                    if self.point_in_selection(cx, cy) {
                        let s = self.sel.unwrap();
                        let mut tiles = Vec::with_capacity((s.w * s.h) as usize);
                        for y in 0..s.h {
                            for x in 0..s.w {
                                tiles.push(assets.map.get(s.x + x, s.y + y));
                            }
                        }
                        self.drag = Drag::Moving { tiles, w: s.w, h: s.h, gx: cx, gy: cy };
                    } else {
                        self.drag = Drag::Selecting { ax: cx, ay: cy };
                    }
                }
                if !mouse.left {
                    match std::mem::replace(&mut self.drag, Drag::None) {
                        Drag::Selecting { ax, ay } => {
                            let (cx, cy) = self.clamped_cell();
                            let (x, y, w, h) = normalize_rect(ax, ay, cx, cy);
                            self.sel = Some(Selection { x, y, w, h });
                        }
                        Drag::Moving { tiles, w, h, gx, gy } => {
                            let s = self.sel.unwrap();
                            let (cx, cy) = self.clamped_cell();
                            let (nx, ny) = (s.x + (cx - gx), s.y + (cy - gy));
                            for y in 0..h {
                                for x in 0..w {
                                    assets.map.set(s.x + x, s.y + y, 0);
                                }
                            }
                            for y in 0..h {
                                for x in 0..w {
                                    let (dx, dy) = (nx + x, ny + y);
                                    if (0..MAP_W as i32).contains(&dx)
                                        && (0..MAP_H as i32).contains(&dy)
                                    {
                                        assets.map.set(dx, dy, tiles[(y * w + x) as usize]);
                                    }
                                }
                            }
                            self.sel = Some(Selection { x: nx, y: ny, w, h });
                        }
                        // A non-Select drag (e.g. a stale Shape/Pan) is put back untouched.
                        other => self.drag = other,
                    }
                }
            }
```

(`point_in_selection`, `copy_selection`, `delete_selection`, and `paste_at` were all added in the
methods block above.) Remove any remaining dead-code allows on `Clipboard`/`clip`/`Selection`.

- [ ] **Step 4: Run to verify they pass**

Run: `SDL_VIDEODRIVER=dummy SDL_AUDIODRIVER=dummy cargo test -p rico8-console -- map`
Expected: PASS (all clipboard tests green).

- [ ] **Step 5: Format, commit**

(Skip `clippy -D warnings` here — `Drag::Shape` is still unused until Task 6; the gate runs in
Task 8.)

```bash
cargo +nightly fmt --all
git add rico8-console/src/editor/map.rs
git commit -m "$(gimoji -s '✨ Add map clipboard copy/cut/paste, delete, and block move')"
```

---

## Task 6: Fill tool — flood fill + rectangle fill

**Files:**
- Modify: `rico8-console/src/editor/map.rs`

- [ ] **Step 1: Write the failing tests**

Add to `mod tests`:

```rust
#[test]
fn fill_click_flood_fills_the_contiguous_region() {
    let mut ed = MapEditor::new();
    let mut a = Assets::default();
    ed.tool = Tool::Fill;
    ed.brush = 9;
    // Default map is all 0; a single click should flood the whole visible-and-beyond region.
    ed.tick(&press(8 + 1, VIEW_Y + 1), &mut a); // cell (1,0), tile 0
    ed.tick(&rel(8 + 1, VIEW_Y + 1), &mut a); // release on the same cell -> flood
    assert_eq!(a.map.get(1, 0), 9);
    assert_eq!(a.map.get(0, 0), 9, "neighbour flooded");
    assert_eq!(a.map.get(40, 30), 9, "far same-tile cell flooded");
}

#[test]
fn fill_drag_fills_a_rectangle() {
    let mut ed = MapEditor::new();
    let mut a = Assets::default();
    a.map.set(50, 50, 3); // make the map non-uniform so flood wouldn't cover the rect
    ed.tool = Tool::Fill;
    ed.brush = 4;
    ed.tick(&press(1 * 8 + 1, VIEW_Y + 1 * 8 + 1), &mut a); // (1,1)
    ed.tick(&held(2 * 8 + 1, VIEW_Y + 2 * 8 + 1), &mut a); // drag to (2,2)
    ed.tick(&rel(2 * 8 + 1, VIEW_Y + 2 * 8 + 1), &mut a); // release on (2,2) -> rect
    for y in 1..=2 {
        for x in 1..=2 {
            assert_eq!(a.map.get(x, y), 4, "cell ({x},{y}) filled");
        }
    }
    assert_eq!(a.map.get(0, 0), 0, "outside rect untouched");
}
```

- [ ] **Step 2: Run to verify they fail**

Run: `SDL_VIDEODRIVER=dummy SDL_AUDIODRIVER=dummy cargo test -p rico8-console -- map`
Expected: FAIL — no Fill handling.

- [ ] **Step 3: Implement the Fill arm and helpers**

Add a `Tool::Fill` arm to the per-tool `match` in `tick`:

```rust
            Tool::Fill => {
                if mouse.left_pressed && mouse.y >= VIEW_Y && mouse.y <= VIEW_BOTTOM {
                    let (cx, cy) = self.clamped_cell();
                    self.drag = Drag::Shape { ax: cx, ay: cy };
                }
                if !mouse.left {
                    if let Drag::Shape { ax, ay } = std::mem::replace(&mut self.drag, Drag::None) {
                        let (cx, cy) = self.clamped_cell();
                        if (cx, cy) == (ax, ay) {
                            self.flood_fill(assets, ax, ay);
                        } else {
                            let (x, y, w, h) = normalize_rect(ax, ay, cx, cy);
                            for yy in y..y + h {
                                for xx in x..x + w {
                                    assets.map.set(xx, yy, self.brush as u8);
                                }
                            }
                        }
                    }
                }
            }
```

Add the flood-fill method:

```rust
    fn flood_fill(&self, assets: &mut Assets, sx: i32, sy: i32) {
        let target = assets.map.get(sx, sy);
        let brush = self.brush as u8;
        if target == brush {
            return;
        }
        let mut stack = vec![(sx, sy)];
        while let Some((x, y)) = stack.pop() {
            if !(0..MAP_W as i32).contains(&x) || !(0..MAP_H as i32).contains(&y) {
                continue;
            }
            if assets.map.get(x, y) != target {
                continue;
            }
            assets.map.set(x, y, brush);
            stack.extend([(x + 1, y), (x - 1, y), (x, y + 1), (x, y - 1)]);
        }
    }
```

- [ ] **Step 4: Add a rectangle preview while dragging Fill**

So a Fill (or Circle, Task 7) drag shows where it will land, add to `draw` a
`self.draw_shape_preview(fb);` call right after `self.draw_selection(fb);`, and the method:

```rust
    fn draw_shape_preview(&self, fb: &mut Framebuffer) {
        let Drag::Shape { ax, ay } = self.drag else { return };
        let (cx, cy) = self.clamped_cell();
        let (x, y, w, h) = normalize_rect(ax, ay, cx, cy);
        let sx = (x - self.cam_x) * 8;
        let sy = VIEW_Y + (y - self.cam_y) * 8;
        fb.rect(sx, sy, sx + w * 8 - 1, sy + h * 8 - 1, col::WHITE);
    }
```

- [ ] **Step 5: Run to verify they pass**

Run: `SDL_VIDEODRIVER=dummy SDL_AUDIODRIVER=dummy cargo test -p rico8-console -- map`
Expected: PASS.

- [ ] **Step 6: Format, commit**

```bash
cargo +nightly fmt --all
git add rico8-console/src/editor/map.rs
git commit -m "$(gimoji -s '✨ Add the map fill tool (flood and rectangle)')"
```

---

## Task 7: Circle tool — ellipse outline

**Files:**
- Modify: `rico8-console/src/editor/map.rs`

- [ ] **Step 1: Write the failing test**

Add to `mod tests`:

```rust
#[test]
fn circle_drag_stamps_an_ellipse_outline() {
    let mut ed = MapEditor::new();
    let mut a = Assets::default();
    ed.tool = Tool::Circle;
    ed.brush = 6;
    // Bounding box cells (2,2)..(6,6): a 5x5 box centred at (4,4).
    ed.tick(&press(2 * 8 + 1, VIEW_Y + 2 * 8 + 1), &mut a);
    ed.tick(&held(6 * 8 + 1, VIEW_Y + 6 * 8 + 1), &mut a);
    ed.tick(&rel(6 * 8 + 1, VIEW_Y + 6 * 8 + 1), &mut a);
    // Outline touches the box-edge midpoints...
    assert_eq!(a.map.get(4, 2), 6, "top midpoint on outline");
    assert_eq!(a.map.get(2, 4), 6, "left midpoint on outline");
    assert_eq!(a.map.get(6, 4), 6, "right midpoint on outline");
    assert_eq!(a.map.get(4, 6), 6, "bottom midpoint on outline");
    // ...and the centre stays empty (outline only).
    assert_eq!(a.map.get(4, 4), 0, "centre not filled");
}
```

- [ ] **Step 2: Run to verify it fails**

Run: `SDL_VIDEODRIVER=dummy SDL_AUDIODRIVER=dummy cargo test -p rico8-console circle_drag -- --nocapture`
Expected: FAIL — Circle does nothing yet.

- [ ] **Step 3: Implement the Circle arm and the ellipse stamper**

Add a `Tool::Circle` arm to the per-tool `match` in `tick` (mirrors Fill's drag, applies an
ellipse outline on release):

```rust
            Tool::Circle => {
                if mouse.left_pressed && mouse.y >= VIEW_Y && mouse.y <= VIEW_BOTTOM {
                    let (cx, cy) = self.clamped_cell();
                    self.drag = Drag::Shape { ax: cx, ay: cy };
                }
                if !mouse.left {
                    if let Drag::Shape { ax, ay } = std::mem::replace(&mut self.drag, Drag::None) {
                        let (cx, cy) = self.clamped_cell();
                        let (x, y, w, h) = normalize_rect(ax, ay, cx, cy);
                        self.stamp_ellipse(assets, x, y, w, h);
                    }
                }
            }
```

`Tool::Fill` also produces a `Drag::Shape`; that is fine because each tool's own arm consumes the
release. Add the ellipse stamper (samples the parametric ellipse and plots the brush per cell):

```rust
    fn stamp_ellipse(&self, assets: &mut Assets, x: i32, y: i32, w: i32, h: i32) {
        let brush = self.brush as u8;
        // Centre and radii reach the cell centres on the box edges, so the
        // outline touches the bounding box (radius (n-1)/2 for an n-cell span).
        let (cx, cy) = (x as f32 + (w - 1) as f32 / 2.0, y as f32 + (h - 1) as f32 / 2.0);
        let (rx, ry) = ((w - 1) as f32 / 2.0, (h - 1) as f32 / 2.0);
        // Walk the perimeter by angle; overlapping samples just re-write a cell.
        let steps = ((w + h) * 4).max(16);
        for s in 0..steps {
            let t = std::f32::consts::TAU * s as f32 / steps as f32;
            let px = (cx + rx * t.cos()).round() as i32;
            let py = (cy + ry * t.sin()).round() as i32;
            if (0..MAP_W as i32).contains(&px) && (0..MAP_H as i32).contains(&py) {
                assets.map.set(px, py, brush);
            }
        }
    }
```

(`steps` is `i32`; the loop `0..steps` and `s as f32` are fine. This parametric sampler passes
the 5×5 midpoint test; for very large ellipses it may leave 1-cell gaps — acceptable for a tile
outline, and `steps` scales with `w + h` to keep it dense.)

- [ ] **Step 4: Run to verify it passes**

Run: `SDL_VIDEODRIVER=dummy SDL_AUDIODRIVER=dummy cargo test -p rico8-console circle_drag`
Expected: PASS. Then run the whole map suite:
`SDL_VIDEODRIVER=dummy SDL_AUDIODRIVER=dummy cargo test -p rico8-console -- map` → PASS.

- [ ] **Step 5: Format, commit**

```bash
cargo +nightly fmt --all
git add rico8-console/src/editor/map.rs
git commit -m "$(gimoji -s '✨ Add the map circle tool (ellipse outline)')"
```

---

## Task 8: Pan tool — drag to scroll

**Files:**
- Modify: `rico8-console/src/editor/map.rs`

- [ ] **Step 1: Write the failing test**

Add to `mod tests`:

```rust
#[test]
fn pan_drag_scrolls_the_camera() {
    let mut ed = MapEditor::new();
    let mut a = Assets::default();
    ed.tool = Tool::Pan;
    ed.cam_x = 20;
    ed.cam_y = 20;
    // Press at x=80, drag left to x=40 (delta -40 px = -5 cells) -> cam_x += 5.
    ed.tick(&press(80, VIEW_Y + 40), &mut a);
    ed.tick(&held(40, VIEW_Y + 16), &mut a); // dx -40 -> +5 cells; dy -24 -> +3 cells
    assert_eq!(ed.cam_x, 25);
    assert_eq!(ed.cam_y, 23);
}
```

- [ ] **Step 2: Run to verify it fails**

Run: `SDL_VIDEODRIVER=dummy SDL_AUDIODRIVER=dummy cargo test -p rico8-console pan_drag -- --nocapture`
Expected: FAIL — Pan does nothing.

- [ ] **Step 3: Implement the Pan arm**

Add a `Tool::Pan` arm to the per-tool `match` in `tick`, then **remove the trailing `_ => {}`
arm** — all six `Tool` variants are now handled, so the wildcard is unreachable and
`unreachable_patterns` (warn-by-default) would fail the `-D warnings` gate. The Pan arm matches
`self.drag` binding only `Copy` (`i32`) fields, so it does not move the enum:

```rust
            Tool::Pan => {
                if mouse.left_pressed {
                    self.drag = Drag::Panning {
                        amx: mouse.x,
                        amy: mouse.y,
                        acx: self.cam_x,
                        acy: self.cam_y,
                    };
                }
                if let Drag::Panning { amx, amy, acx, acy } = self.drag {
                    if mouse.left {
                        self.cam_x = (acx + (amx - mouse.x) / 8)
                            .clamp(0, MAP_W as i32 - VIEW_TILES_X);
                        self.cam_y = (acy + (amy - mouse.y) / 8)
                            .clamp(0, MAP_H as i32 - VIEW_TILES_Y);
                    } else {
                        self.drag = Drag::None;
                    }
                }
            }
```

- [ ] **Step 4: Run to verify it passes**

Run: `SDL_VIDEODRIVER=dummy SDL_AUDIODRIVER=dummy cargo test -p rico8-console pan_drag`
Expected: PASS.

- [ ] **Step 5: Full verification (CI parity)**

Run:
```
cargo +nightly fmt --all
SDL_VIDEODRIVER=dummy SDL_AUDIODRIVER=dummy cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
```
Expected: all PASS, zero warnings. There must be **no `#[allow(dead_code)]` anywhere** — every
`Tool`, `Drag` variant, `Selection`/`Clipboard` field, and `frame` is now constructed and read,
and the `_ => {}` match arm has been removed.

- [ ] **Step 6: Commit**

```bash
git add rico8-console/src/editor/map.rs
git commit -m "$(gimoji -s '✨ Add the map pan tool (drag to scroll)')"
```

---

## Manual verification

After Task 8, boot the console and exercise the editor by hand (the `/run` skill or):

```
cargo console -- examples/platformer
```

Then press the map tab and confirm: tool icons highlight on click and via `d/t/s/h/f/c`; the
hover box and status line track the cursor; Select shows marching ants and `Ctrl+C`/Paste round-
trips a block; Fill flood/rect work; Circle draws an outline; Pan and the arrow keys scroll; the
four page dots reach all 256 sprites; and the separator cleanly divides the map from the sheet.

## Self-review notes (coverage vs spec)

- Layout/constants → Task 2. Toolbar + 6 PICO-8 icons → Tasks 1–2. Full-page sheet + 4 page dots
  + paging mod 4 → Task 2. Brush preview → Task 2. Separator → Task 2.
- Draw + hover box + status (hover and selection forms) → Tasks 3–4. Marching ants + frame
  counter → Task 4. Clipboard + copy/cut/delete/paste + move → Task 5. Fill (flood + rect) →
  Task 6. Circle outline → Task 7. Pan → Task 8.
- Shared `Icon8`/`draw_icon8`/`ICON_PENCIL` + sprite-editor pencil upgrade → Task 1.
- Dropped (per spec): eyedropper tool, filled circle, numbered tabs, `Ctrl+V` — none planned.
