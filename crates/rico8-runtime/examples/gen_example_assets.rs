//! Generates `assets.rico8` for the example carts in `examples/`.
//!
//! Run from the repo root after changing anything here:
//!
//! ```text
//! cargo run -p rico8-runtime --example gen_example_assets
//! ```
//!
//! The generated files are committed: they are game data, like any pixel
//! art, just born from code instead of the sprite editor.

use rico8_runtime::assets::{Assets, MusicPattern, Note};
use rico8_runtime::project::encode_assets;
use std::path::Path;

/// Paint an 8x8 sprite from 8 strings; chars index the palette in hex,
/// `.` is transparent (color 0).
fn sprite(assets: &mut Assets, n: u32, rows: [&str; 8]) {
    let ox = (n as i32 % 16) * 8;
    let oy = (n as i32 / 16) * 8;
    for (y, row) in rows.iter().enumerate() {
        for (x, c) in row.chars().enumerate() {
            let color = match c {
                '.' => 0,
                _ => c.to_digit(16).expect("hex digit") as u8,
            };
            assets.sprites.set(ox + x as i32, oy + y as i32, color);
        }
    }
}

fn base_assets() -> Assets {
    let mut a = Assets::default();

    // Sprite 1/2: a friendly slime, two walk frames.
    sprite(
        &mut a,
        1,
        [
            "........", "..bbbb..", ".bbbbbb.", ".b7b7bb.", ".b0b0bb.", ".bbbbbb.", ".b.bb.b.",
            "........",
        ],
    );
    sprite(
        &mut a,
        2,
        [
            "........", "..bbbb..", ".bbbbbb.", ".b7b7bb.", ".b0b0bb.", ".bbbbbb.", "b..bb..b",
            "........",
        ],
    );
    // Sprite 3: coin.
    sprite(
        &mut a,
        3,
        [
            "........", "..aaaa..", ".aa77aa.", ".a7aaaa.", ".aaaaa9.", ".aa99a9.", "..a999..",
            "........",
        ],
    );
    // Sprite 16: grass block (solid).
    sprite(
        &mut a,
        16,
        [
            "bbbbbbbb", "b3b3b3b3", "44444444", "44444944", "44944444", "44444444", "44449444",
            "44444444",
        ],
    );
    // Sprite 17: stone brick (solid).
    sprite(
        &mut a,
        17,
        [
            "66666666", "65656556", "66666666", "56565665", "66666666", "65656556", "66666666",
            "55555555",
        ],
    );
    // Sprite 18: bush (decoration).
    sprite(
        &mut a,
        18,
        [
            "........", "...33...", "..3bb3..", ".3bbbb3.", ".3bb3b3.", "3bbbbbb3", "33333333",
            "........",
        ],
    );
    // Sprite 19: cloud (decoration).
    sprite(
        &mut a,
        19,
        [
            "........", "..777...", ".77777..", "7777777.", "77777777", ".777777.", "........",
            "........",
        ],
    );
    // Flag 0 marks solid tiles.
    a.sprites.set_flag(16, 0, true);
    a.sprites.set_flag(17, 0, true);

    // A 32x16 level: floor, platforms, coins, decoration.
    for x in 0..32 {
        a.map.set(x, 14, 16);
        a.map.set(x, 15, 17);
    }
    for x in [6, 7, 8, 14, 15, 20, 21, 22, 27, 28] {
        a.map.set(x, 10, 17);
    }
    for x in [10, 11, 12, 24, 25] {
        a.map.set(x, 7, 17);
    }
    for (x, y) in [
        (7, 9),
        (15, 9),
        (21, 9),
        (11, 6),
        (24, 6),
        (4, 13),
        (30, 13),
    ] {
        a.map.set(x, y, 3); // coins
    }
    for x in [2, 12, 18, 26] {
        a.map.set(x, 13, 18); // bushes
    }
    for (x, y) in [(3, 2), (13, 3), (22, 1), (29, 4)] {
        a.map.set(x, y, 19); // clouds
    }

    // --- SFX ---
    // 0: jump — quick rising saw.
    {
        let s = &mut a.sfx[0];
        s.speed = 3;
        for (i, n) in s.notes.iter_mut().enumerate().take(8) {
            *n = Note {
                pitch: (20 + i * 3) as u8,
                wave: 2,
                volume: (6 - i / 2) as u8,
                effect: 1,
            };
        }
    }
    // 1: coin — two-note square chime.
    {
        let s = &mut a.sfx[1];
        s.speed = 4;
        s.notes[0] = Note {
            pitch: 45,
            wave: 3,
            volume: 6,
            effect: 0,
        };
        s.notes[1] = Note {
            pitch: 50,
            wave: 3,
            volume: 6,
            effect: 0,
        };
        s.notes[2] = Note {
            pitch: 50,
            wave: 3,
            volume: 4,
            effect: 5,
        };
    }
    // 2: laser — fast descending pulse.
    {
        let s = &mut a.sfx[2];
        s.speed = 2;
        for (i, n) in s.notes.iter_mut().enumerate().take(12) {
            *n = Note {
                pitch: (55 - i * 3) as u8,
                wave: 4,
                volume: 5,
                effect: 3,
            };
        }
    }
    // 3: hurt — noise thud.
    {
        let s = &mut a.sfx[3];
        s.speed = 5;
        for (i, n) in s.notes.iter_mut().enumerate().take(6) {
            *n = Note {
                pitch: (24 - i * 3) as u8,
                wave: 6,
                volume: (6 - i) as u8,
                effect: 5,
            };
        }
    }

    // --- Music: an 8-note melody, a bass line and a hat pattern. ---
    // 8: melody (square).
    {
        let s = &mut a.sfx[8];
        s.speed = 12;
        let tune = [
            36, 0, 40, 0, 43, 40, 36, 0, 38, 0, 41, 0, 45, 41, 38, 0, 36, 0, 40, 0, 43, 40, 48, 0,
            47, 45, 43, 41, 40, 38, 36, 0,
        ];
        for (i, &p) in tune.iter().enumerate() {
            if p > 0 {
                s.notes[i] = Note {
                    pitch: p,
                    wave: 3,
                    volume: 5,
                    effect: 0,
                };
            }
        }
    }
    // 9: bass (triangle).
    {
        let s = &mut a.sfx[9];
        s.speed = 12;
        for i in 0..32 {
            let root = [12u8, 12, 14, 14, 12, 12, 16, 14][(i / 4) % 8];
            if i % 2 == 0 {
                s.notes[i] = Note {
                    pitch: root,
                    wave: 0,
                    volume: 5,
                    effect: 0,
                };
            }
        }
    }
    // 10: hats (noise).
    {
        let s = &mut a.sfx[10];
        s.speed = 12;
        for i in 0..32 {
            if i % 4 == 2 {
                s.notes[i] = Note {
                    pitch: 50,
                    wave: 6,
                    volume: 2,
                    effect: 5,
                };
            }
        }
    }
    // Pattern 0 loops back to itself.
    a.music[0] = MusicPattern {
        channels: [Some(8), Some(9), Some(10), None],
        loop_start: true,
        loop_back: true,
        stop_at_end: false,
    };

    a
}

fn main() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples");
    for (dir, author) in [
        ("sprite_move", "rico-8"),
        ("platformer", "rico-8"),
        ("map_demo", "rico-8"),
        ("sfx_demo", "rico-8"),
        ("music_demo", "rico-8"),
    ] {
        let mut assets = base_assets();
        assets.meta.name = dir.replace('_', " ");
        assets.meta.author = author.into();
        let path = root.join(dir).join("assets.rico8");
        std::fs::write(&path, encode_assets(&assets).expect("encode")).expect("write");
        println!("wrote {}", path.display());
    }
}
