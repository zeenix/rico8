//! The cart shelf: scanning a directory for carts and drawing the picker screen.

use anyhow::{Context, Result};
use rico8_runtime::{cart, fb::Framebuffer, palette::col};
use std::path::{Path, PathBuf};

/// All RICO-8 carts in a directory, sorted by file name.
pub fn scan_carts(dir: &Path) -> Result<Vec<PathBuf>> {
    let mut carts: Vec<PathBuf> = std::fs::read_dir(dir)
        .with_context(|| format!("reading {}", dir.display()))?
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|e| e == "png"))
        .filter(|p| {
            std::fs::read(p)
                .map(|bytes| cart::is_cart(&bytes))
                .unwrap_or(false)
        })
        .collect();
    carts.sort();
    Ok(carts)
}

/// Render the cart shelf in console style.
pub fn draw_picker(dir: &Path, carts: &[PathBuf], sel: usize, frame: u32) -> Framebuffer {
    let mut fb = Framebuffer::new();
    fb.cls(col::BLACK);
    for (i, c) in [8u8, 9, 10, 11, 12, 13, 14, 15].iter().enumerate() {
        fb.rectfill(2 + i as i32 * 6, 2, 6 + i as i32 * 6, 5, *c);
    }
    fb.print("rico-8 carts", 2, 10, col::WHITE);

    if carts.is_empty() {
        fb.print("no carts found in", 2, 30, col::LIGHT_GREY);
        let dir = dir.to_string_lossy();
        let tail: String = dir
            .chars()
            .rev()
            .take(30)
            .collect::<Vec<_>>()
            .iter()
            .rev()
            .collect();
        fb.print(&tail, 2, 38, col::LIGHT_GREY);
        fb.print("copy .png carts here!", 2, 54, col::ORANGE);
    }

    // The selectable list is the carts followed by a trailing "-- quit --" entry, so the
    // window of rows and the selection index both span `carts.len() + 1`.
    let total = carts.len() + 1;
    let rows = 13usize;
    let top = sel.saturating_sub(rows / 2).min(total.saturating_sub(rows));
    for row in 0..rows.min(total.saturating_sub(top)) {
        let i = top + row;
        let y = 22 + row as i32 * 7;
        let is_quit = i == carts.len();
        let name = if is_quit {
            "-- quit --".to_string()
        } else {
            carts[i]
                .file_stem()
                .map(|s| s.to_string_lossy().into_owned())
                .unwrap_or_default()
        };
        if i == sel {
            fb.rectfill(0, y - 1, 127, y + 5, col::DARK_BLUE);
            // Blinking chevron, console style.
            if (frame / 8).is_multiple_of(2) {
                fb.print(">", 2, y, col::RED);
            }
            fb.print(&name, 8, y, if is_quit { col::RED } else { col::WHITE });
        } else {
            fb.print(
                &name,
                8,
                y,
                if is_quit { col::RED } else { col::LIGHT_GREY },
            );
        }
    }
    // Controls, along the bottom of the shelf.
    fb.print("up/dn:pick   o/x:select", 2, 114, col::DARK_GREY);
    fb.print("in game: hold o+x to exit", 2, 121, col::DARK_GREY);
    fb
}

/// Draw the fps meter in the top-left: measured frames per second over the
/// cart's target rate, e.g. `60/60`.
pub fn draw_fps_overlay(fb: &mut Framebuffer, measured: f32, target: u32) {
    let text = format!("{}/{}", measured.round() as u32, target);
    let w = text.len() as i32 * 4 + 1;
    fb.rectfill(0, 0, w, 6, col::BLACK);
    fb.print(&text, 1, 1, col::YELLOW);
}

#[cfg(test)]
mod tests {
    use super::*;
    use rico8_runtime::{assets::Assets, cart::Cart};

    #[test]
    fn scan_finds_only_carts_sorted() {
        let dir = std::env::temp_dir().join(format!("rico8_scan_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let cart = Cart {
            wasm: b"\0asm\x01\0\0\0".to_vec(),
            assets: Assets::default(),
            source: None,
        };
        let png = cart::encode(&cart).unwrap();
        std::fs::write(dir.join("b_game.png"), &png).unwrap();
        std::fs::write(dir.join("a_game.png"), &png).unwrap();
        // Decoys: a non-cart png and a text file.
        std::fs::write(dir.join("photo.png"), b"\x89PNG\r\n\x1a\nnotacart").unwrap();
        std::fs::write(dir.join("readme.txt"), b"hi").unwrap();

        let found = scan_carts(&dir).unwrap();
        let names: Vec<_> = found
            .iter()
            .map(|p| p.file_name().unwrap().to_string_lossy().into_owned())
            .collect();
        assert_eq!(names, ["a_game.png", "b_game.png"]);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn picker_draws_selection() {
        let carts = vec![PathBuf::from("one.png"), PathBuf::from("two.png")];
        let fb = draw_picker(Path::new("."), &carts, 1, 0);
        // Second row has the selection bar (dark blue).
        assert_eq!(fb.pget(0, 29), col::DARK_BLUE);
    }
}
