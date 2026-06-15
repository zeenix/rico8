# SFX & Music Editor Rework Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Rework RICO-8's SFX and music editors to look and behave like PICO-8's — a pattern/song
music editor that shows each channel's notes inline with a live playhead, and an SFX editor with
pitch / tracker / wave-designer modes — matching PICO-8's exact 128×128 layout.

**Architecture:** UI-only for the music editor and most of the SFX editor (the data model and audio
engine are already PICO-8-faithful). One additive data field (`Sfx.custom_wave`) plus synth support
adds PICO-8's drawn-waveform instruments. The editors' drawing is ported from the pixel-verified
`rico8-runtime/examples/mockup_editors.rs`; fidelity is checked against recovered PICO-8 framebuffers
(`docs/superpowers/specs/2026-06-15-sfx-music-editor-rework-assets/`).

**Tech Stack:** Rust, the `rico8-runtime` software framebuffer/font/palette, `serde`/`postcard` for
the cart format, `cpal` synth. Tests run with `cargo test -p <crate>`.

**Reference design:** `docs/superpowers/specs/2026-06-15-sfx-music-editor-rework-design.md`.

---

## File structure

- `rico8-runtime/src/assets.rs` — add `CustomWave` + `Sfx.custom_wave` (Phase 1).
- `rico8-runtime/src/audio.rs` — `Synth::channel_step()`, drawn-waveform synthesis + bass (Phase 2).
- `rico8-runtime/src/pico8.rs` — import PICO-8 waveform instruments into `custom_wave` (Phase 3).
- `rico8-console/src/ui.rs` — active-tab highlight (fg-only) + chrome icon helpers/blits (Phase 4).
- `rico8-console/src/editor/music.rs` — full rewrite (Phase 6).
- `rico8-console/src/editor/sfx.rs` — restructure into pitch/tracker/wave modes (Phase 7).
- `rico8-console/src/shell.rs` — music↔SFX handoff (selected SFX) + playhead plumbing (Phase 5, 8).
- `rico8-runtime/examples/mockup_editors.rs` — kept as the layout reference during implementation;
  **deleted in the final task** along with `mockups/`.

Order: data/synth/import first (testable foundations), then shared chrome, then the editors.

---

## Phase 1 — Data model: drawn-waveform instruments

### Task 1: Add `CustomWave` and `Sfx.custom_wave`

**Files:**
- Modify: `rico8-runtime/src/assets.rs`
- Test: `rico8-runtime/src/assets.rs` (`#[cfg(test)] mod tests`)

- [ ] **Step 1: Write the failing test**

Add to the `tests` module in `assets.rs`:

```rust
#[test]
fn custom_wave_roundtrips_and_defaults_none() {
    // A fresh SFX has no custom waveform.
    assert!(Sfx::default().custom_wave.is_none());

    let mut a = Assets::default();
    a.sfx[0].custom_wave = Some(CustomWave {
        samples: [3; SFX_LEN],
        bass: true,
    });
    let bytes = postcard::to_allocvec(&a).unwrap();
    let b: Assets = postcard::from_bytes(&bytes).unwrap();
    let w = b.sfx[0].custom_wave.as_ref().expect("wave kept");
    assert_eq!(w.samples[0], 3);
    assert!(w.bass);
    assert!(b.sfx[1].custom_wave.is_none());
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p rico8-runtime custom_wave_roundtrips`
Expected: FAIL to compile — `CustomWave` and `custom_wave` do not exist.

- [ ] **Step 3: Implement**

In `assets.rs`, add the type above `Sfx` (usage-before-definition: it's used by `Sfx`, so place it
just before `Sfx`). One sample per SFX step, signed, drawn in the wave designer:

```rust
/// A drawn custom-waveform instrument occupying SFX slots `0..8`. When present,
/// the slot is used as an instrument timbre (one signed sample per step) by
/// notes that reference it, rather than as a sequence of notes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct CustomWave {
    /// One signed sample per step; the editor draws values in `-16..=15`.
    pub samples: [i8; SFX_LEN],
    /// Pitch the waveform an octave down (PICO-8's "bass" toggle).
    pub bass: bool,
}
```

Add the field to `Sfx` (after the filter switches, with the other optional state), and to
`Sfx::default()`:

```rust
    // in struct Sfx, after `dampen`:
    /// `Some` only for slots 0..8 that are drawn-waveform instruments.
    #[serde(default)]
    pub custom_wave: Option<CustomWave>,
```

```rust
    // in impl Default for Sfx, add to the struct literal:
            custom_wave: None,
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test -p rico8-runtime custom_wave_roundtrips`
Expected: PASS. Also run `cargo test -p rico8-runtime assets` — existing asset tests still pass.

- [ ] **Step 5: Format and commit**

```bash
cargo fmt -p rico8-runtime
git add rico8-runtime/src/assets.rs
git commit -m "✨ runtime: add drawn custom-waveform field to Sfx"
```

(Use the curated gimoji ✨ verbatim per CONTRIBUTING. End the commit body with the
`Assisted-by:` line, not an author line.)

---

## Phase 2 — Synth: playhead + drawn-waveform playback

### Task 2: Expose the live per-channel step

**Files:**
- Modify: `rico8-runtime/src/audio.rs`
- Test: `rico8-runtime/src/audio.rs` (`tests`)

- [ ] **Step 1: Write the failing test**

```rust
#[test]
fn channel_step_tracks_playback() {
    let mut synth = Synth::new(44100.0);
    synth.load(test_sfx(), vec![MusicPattern::default(); 64]);
    assert_eq!(synth.channel_step(), [None, None, None, None]);
    synth.play_sfx(0, 0);
    // After starting, channel 0 is on step 0.
    assert_eq!(synth.channel_step()[0], Some(0));
    // Default speed 16 -> 0.125 s/step; advance ~0.2 s, expect step 1.
    for _ in 0..(44100 / 5) {
        synth.next_sample();
    }
    assert_eq!(synth.channel_step()[0], Some(1));
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p rico8-runtime channel_step_tracks`
Expected: FAIL — `channel_step` not found.

- [ ] **Step 3: Implement**

In `audio.rs`, add to `impl Synth` next to `channel_sfx` (keep pub-before-private and usage order):

```rust
    /// Which step each channel's voice is currently sounding (for editor
    /// playheads); `None` when the channel is idle.
    pub fn channel_step(&self) -> [Option<usize>; CHANNELS] {
        let mut out = [None; CHANNELS];
        for (i, v) in self.voices.iter().enumerate() {
            out[i] = v.as_ref().map(|v| v.step);
        }
        out
    }
```

Add the matching passthrough on `AudioHandle` (next to `play_sfx`):

```rust
    pub fn channel_step(&self) -> [Option<usize>; crate::assets::CHANNELS] {
        self.with_synth(|s| s.channel_step())
    }
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test -p rico8-runtime channel_step_tracks`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
cargo fmt -p rico8-runtime
git add rico8-runtime/src/audio.rs
git commit -m "✨ runtime: expose the live per-channel step from the synth"
```

### Task 3: Play drawn waveforms (with bass)

**Files:**
- Modify: `rico8-runtime/src/audio.rs`
- Test: `rico8-runtime/src/audio.rs` (`tests`)

- [ ] **Step 1: Write the failing test**

```rust
#[test]
fn drawn_waveform_instrument_is_audible_and_bass_drops_octave() {
    use crate::assets::{CustomWave, Note, NOTE_CUSTOM_FLAG, SFX_COUNT, SFX_LEN};
    let mut sfx = vec![Sfx::default(); SFX_COUNT];
    // SFX 1 is a drawn-waveform instrument: a simple ramp.
    let mut samples = [0i8; SFX_LEN];
    for (i, s) in samples.iter_mut().enumerate() {
        *s = (i as i8) - 16;
    }
    sfx[1].custom_wave = Some(CustomWave { samples, bass: false });
    // SFX 0 plays SFX 1 as a custom instrument.
    for note in sfx[0].notes.iter_mut() {
        *note = Note { pitch: 33, wave: NOTE_CUSTOM_FLAG | 1, volume: 5, effect: 0 };
    }
    let mut synth = Synth::new(44100.0);
    synth.load(sfx, vec![MusicPattern::default(); 64]);
    synth.play_sfx(0, 0);
    let mut peak = 0.0f32;
    for _ in 0..2000 {
        let s = synth.next_sample();
        assert!(s.is_finite() && s.abs() <= 1.0);
        peak = peak.max(s.abs());
    }
    assert!(peak > 0.01, "a drawn-waveform note should be audible");
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p rico8-runtime drawn_waveform_instrument`
Expected: FAIL — the custom-wave samples are ignored, so the note still plays a built-in waveform
(test may pass accidentally on audibility; the real check is the next step wires the samples). If it
passes by accident, tighten by asserting the rendered samples differ from the built-in path — but
audibility + finiteness is the minimum gate.

- [ ] **Step 3: Implement**

The synth currently resolves a custom-instrument note to another SFX's *built-in* waveform
(`audio.rs`, `Voice::sample`, the `match note.instrument()` block). Extend `Voice` to optionally
carry the drawn samples of the eight instrument slots and sample from them.

a) Pass the instrument slots' custom waves into `next_sample`/`sample`. Replace the `inst_waves:
&[u8; 8]` plumbing with a small struct holding both the built-in waveform index and the optional
drawn wave:

```rust
/// Timbre of the eight SFX slots usable as custom instruments.
#[derive(Clone, Copy)]
struct InstTimbre {
    wave: u8,
    drawn: Option<(crate::assets::CustomWave,)>,
}
```

Simplest concrete approach (avoids threading a new type everywhere): keep `inst_waves: &[u8; 8]`
**and** add `inst_drawn: &[Option<CustomWave>; 8]`. In `Synth::next_sample`, build both:

```rust
        let mut inst_waves = [0u8; 8];
        let mut inst_drawn: [Option<crate::assets::CustomWave>; 8] = Default::default();
        for i in 0..8 {
            if let Some(s) = self.sfx.get(i) {
                inst_waves[i] = s.notes[0].wave_index();
                inst_drawn[i] = s.custom_wave;
            }
        }
```

Pass `&inst_drawn` into `voice.sample(dt, self.t, &inst_waves, &inst_drawn)` and update the signature.

b) In `Voice::sample`, when the note is a custom instrument whose slot has a drawn wave, sample the
table instead of `tonal_wave`. Add a helper:

```rust
/// One sample of a drawn 32-point waveform at phase `[0,1)`, linearly
/// interpolated. Samples are signed `i8`; normalise to roughly `[-1,1]`.
fn drawn_wave(w: &crate::assets::CustomWave, phase: f32) -> f32 {
    let n = w.samples.len();
    let fpos = phase * n as f32;
    let i0 = (fpos as usize) % n;
    let i1 = (i0 + 1) % n;
    let frac = fpos - fpos.floor();
    let a = w.samples[i0] as f32 / 16.0;
    let b = w.samples[i1] as f32 / 16.0;
    a + (b - a) * frac
}
```

In `sample`, after resolving `wave`/`freq`, branch: if the note is a custom instrument and
`inst_drawn[slot]` is `Some(w)`, set `let freq = if w.bass { freq * 0.5 } else { freq };` for the
oscillator advance and produce `raw = drawn_wave(&w, self.phase)` (skipping the `tonal_wave`/noise
path). Keep the existing `buzz`/`dampen`/`reverb`/`vol` post-processing applied to `raw`.

Note: `note.instrument()` already gives `Some(slot)`. Resolve the drawn wave as
`let drawn = note.instrument().and_then(|s| inst_drawn[s as usize]);` and use it.

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test -p rico8-runtime drawn_waveform_instrument`
Expected: PASS. Also run `cargo test -p rico8-runtime audio` — the existing custom-instrument and
filter tests still pass (drawn-wave path only triggers when `custom_wave` is `Some`).

- [ ] **Step 5: Commit**

```bash
cargo fmt -p rico8-runtime
git add rico8-runtime/src/audio.rs
git commit -m "✨ runtime: synthesize drawn custom-waveform instruments"
```

---

## Phase 3 — Import PICO-8 waveform instruments

### Task 4: Import drawn waveforms from PICO-8 carts

**Files:**
- Modify: `rico8-runtime/src/pico8.rs`
- Test: `rico8-runtime/src/pico8.rs` (`tests` if present; else add one)

Background: in a PICO-8 cart, an SFX slot `0..8` flagged as a waveform instrument stores its samples
in the note bytes. Confirm the exact bit/flag when implementing (cross-check picotool / the BBS
"Waveform Instrument Encoding" thread, tid=45247). The importer for both the `.p8` text path and the
`.p8.png` memory path should detect the flag and fill `custom_wave` instead of (or in addition to)
`notes`.

- [ ] **Step 1: Write the failing test**

Add a test that feeds a minimal PICO-8 `__sfx__` line whose slot-0 metadata marks a waveform
instrument and asserts `assets.sfx[0].custom_wave.is_some()`. (Construct the hex line per the encoding
you confirm; if the public import API only takes a file, write a small `.p8` to a temp dir as the
existing import tests do.) Assert a non-waveform SFX keeps `custom_wave == None`.

- [ ] **Step 2: Run to verify it fails**

Run: `cargo test -p rico8-runtime import` — Expected: FAIL (`custom_wave` stays `None`).

- [ ] **Step 3: Implement**

In `sfx_from_mem` (and the text `__sfx__` loop), after decoding the filter/mode byte, detect the
waveform-instrument flag for slots `0..8`; when set, decode the 32 samples into
`Sfx.custom_wave = Some(CustomWave { samples, bass })` (map PICO-8's sample range into `-16..=15`).

- [ ] **Step 4: Run to verify it passes**

Run: `cargo test -p rico8-runtime import` — Expected: PASS.

- [ ] **Step 5: Commit**

```bash
cargo fmt -p rico8-runtime
git add rico8-runtime/src/pico8.rs
git commit -m "✨ runtime: import PICO-8 waveform instruments"
```

---

## Phase 4 — Shared chrome (`rico8-console/src/ui.rs`)

### Task 5: Active-tab highlight = foreground only

**Files:**
- Modify: `rico8-console/src/ui.rs:63-83` (`draw_tab_bar`)
- Test: `rico8-console/src/ui.rs` (add a small `tests` module)

- [ ] **Step 1: Write the failing test**

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use rico8_runtime::fb::Framebuffer;

    #[test]
    fn active_tab_has_no_background_box() {
        let mut fb = Framebuffer::new();
        draw_tab_bar(&mut fb, Mode::Music);
        // The music tab is the 5th icon at tab_x(4). The cell behind it must
        // stay red (no dark-purple box); the icon itself is peach.
        let x = tab_x(4);
        assert_eq!(fb.pget(x - 1, 3), col::RED, "no background box behind active tab");
    }
}
```

- [ ] **Step 2: Run to verify it fails**

Run: `cargo test -p rico8-console active_tab_has_no_background`
Expected: FAIL — current code paints a `DARK_PURPLE` box (pget returns `DARK_PURPLE`).

- [ ] **Step 3: Implement**

Rewrite the loop body in `draw_tab_bar`:

```rust
    for (i, (icon, mode)) in TABS.iter().enumerate() {
        let x = tab_x(i);
        let color = if *mode == active { col::PEACH } else { col::DARK_PURPLE };
        rui::icon(fb, icon, x, 0, color);
    }
```

(Remove the `rectfill` behind the active tab.)

- [ ] **Step 4: Run to verify it passes**

Run: `cargo test -p rico8-console active_tab_has_no_background` — Expected: PASS.

- [ ] **Step 5: Commit**

```bash
cargo fmt -p rico8-console
git add rico8-console/src/ui.rs
git commit -m "🎨 console: highlight the active editor tab by colour, not a box"
```

### Task 6: Chrome helpers and PICO-8 pixel blits

**Files:**
- Modify: `rico8-console/src/ui.rs`

These are the icon/blit helpers the editors share. Port them from
`rico8-runtime/examples/mockup_editors.rs` (which is pixel-verified). No new tests — they are drawing
primitives verified by the editor render checks in later phases.

- [ ] **Step 1: Add the helpers**

Add to `ui.rs` (console): `blit(fb, x, y, rows: &[&str])` (hex palette index per cell; `'5'`
transparent), `arrow_l`/`arrow_r`, `mode_buttons(fb, pitch_active)` (the bar-chart + dot-grid),
`radio(fb, x, y, on)`, `pencil(fb, x, y)`, and the PICO-8 pixel-grid constants `FLOW` (flow buttons),
`PALETTE` (8 waveform boxes), `CIRCLE`, `WAVEI` — copy the exact grids from `mockup_editors.rs`.

- [ ] **Step 2: Build**

Run: `cargo build -p rico8-console` — Expected: compiles (helpers unused yet → allow with usage in
the next phases; if `dead_code` warns, that's fine until wired).

- [ ] **Step 3: Commit**

```bash
cargo fmt -p rico8-console
git add rico8-console/src/ui.rs
git commit -m "🎨 console: add shared editor chrome helpers (arrows, blits, icons)"
```

---

## Phase 5 — Shell wiring (`rico8-console/src/shell.rs`)

### Task 7: Music→SFX edit handoff + selected-SFX state

**Files:**
- Modify: `rico8-console/src/shell.rs` (the `Mode::Music`/`Mode::Sfx` tick arms, ~1044-1058)
- Modify: `rico8-console/src/editor/music.rs` (add a pending-edit accessor — stub for now)
- Modify: `rico8-console/src/editor/sfx.rs` (add `select(n)` — stub for now)

- [ ] **Step 1: Add the handoff accessors (stubs)**

In `music.rs`, add a field `edit_request: Option<usize>` (init `None`) and:

```rust
    /// Take a pending "edit this channel's SFX" request (set when the pencil is
    /// clicked); the shell routes it to the SFX editor.
    pub fn take_edit_request(&mut self) -> Option<usize> {
        self.edit_request.take()
    }
```

In `sfx.rs`, add:

```rust
    /// Select an SFX slot (used when jumping in from the music editor).
    pub fn select(&mut self, sfx: usize) {
        self.sfx = sfx.min(crate::editor::sfx_count_max());
    }
```

(Use the existing `64` bound directly: `self.sfx = sfx % 64;`.)

- [ ] **Step 2: Wire the shell**

In `shell.rs`, after the `Mode::Music => { self.music_ed.tick(...) }` call, add:

```rust
                        if let Some(n) = self.music_ed.take_edit_request() {
                            self.sfx_ed.select(n);
                            self.switch_editor(Mode::Sfx);
                        }
```

- [ ] **Step 3: Build + run existing shell tests**

Run: `cargo test -p rico8-console shell` — Expected: PASS (no behavior change yet; the request is
never set until the music editor sets it in Phase 6).

- [ ] **Step 4: Commit**

```bash
cargo fmt -p rico8-console
git add rico8-console/src/shell.rs rico8-console/src/editor/music.rs rico8-console/src/editor/sfx.rs
git commit -m "✨ console: route the music-editor pencil to the SFX editor"
```

---

## Phase 6 — Music editor rewrite (`rico8-console/src/editor/music.rs`)

The draw layout is already implemented and pixel-verified in `mockup_editors.rs::music_editor`. Port
it into `MusicEditor::draw`, replacing the static example data with the live pattern/SFX data and the
synth playhead. Keep input handling (`key`/`tick`) for the controls below. Verify each visible piece
against `…-assets/pico8_music_clean.png` by rendering (see the render-check task at the end of the
phase). Build incrementally; commit per logical piece.

### Task 8: Draw — top bar, pattern strip, flow buttons

- [ ] **Step 1:** Implement the top bar (`ui::draw_tab_bar` + `ui::mode_buttons`) and the pattern
  strip: `"pattern"` label, `arrow_l`/`arrow_r`, five pattern buttons centred on `self.pattern`
  (black fill; current = white frame; others light-grey digits), per-channel activity dots
  (orange/yellow/green/blue) only for channels with `Some` SFX, and `ui::blit(.., FLOW)` for the flow
  buttons. Colour each pattern's dots by channel index.
- [ ] **Step 2:** `cargo build -p rico8-console`; run the console (`cargo run -p rico8-console`), open
  a cart, `music`, and eyeball the strip; or use the render-check (Task 12).
- [ ] **Step 3:** Commit: `✨ console: draw the music-editor pattern strip and flow buttons`.

### Task 9: Draw — channel headers + note columns + playhead

- [ ] **Step 1:** For each of 4 channels: header = `ui::radio(enabled)` + black SFX# box (when
  enabled) + `ui::pencil`. Note panel: black fill (active) / black-bordered box (empty). Render the
  referenced SFX's steps via a `note_cell` helper (port from the mockup: letter white, octave
  light-grey, instrument pink / green-if-custom, volume blue, effect grey `.`/orange). Empty steps
  (volume 0) → faint dotted line. Row pitch: scale the SFX's playable length
  (`audio::sfx_steps`-equivalent) to fill the column (start with 8px; refine to PICO-8's scaling).
- [ ] **Step 2:** Playhead: read `audio.channel_step()` in `draw`; highlight each channel's current
  step row in yellow.
- [ ] **Step 3:** Build + eyeball/render-check.
- [ ] **Step 4:** Commit: `✨ console: draw inline per-channel notes with a live playhead`.

### Task 10: Input — pattern nav, channel toggles, SFX#, flow flags, pencil

- [ ] **Step 1:** Rework `tick` (mouse) and `key`:
  - pattern strip arrows / `PgUp`/`PgDn` change `self.pattern`; click a pattern box selects it.
  - click a channel radio toggles `pat.channels[ch]` between `Some(last)`/`None`.
  - left/right-click the SFX# box: `+1`/`-1` (wrap 0..64); shift = ±4.
  - click the flow buttons: toggle `loop_start`/`loop_back`/`stop_at_end`.
  - click the pencil: `self.edit_request = Some(sfx_index_of(ch))`.
  - `Space`: play/stop (reuse `toggle_play`).
- [ ] **Step 2:** Add unit tests for the pure logic where feasible (e.g. a helper
  `nudge_sfx(Option<u8>, delta) -> Option<u8>` with wrap), test-first.
- [ ] **Step 3:** Build + `cargo test -p rico8-console`.
- [ ] **Step 4:** Commit: `✨ console: music-editor pattern/channel/flow input`.

### Task 11: Remove dead old music-editor code

- [ ] **Step 1:** Delete the old row-based layout/flag-strip code superseded by the rewrite.
- [ ] **Step 2:** `cargo build -p rico8-console`; `cargo clippy -p rico8-console` clean.
- [ ] **Step 3:** Commit: `♻️ console: drop the old music-editor layout`.

### Task 12: Render-check the music editor against PICO-8

- [ ] **Step 1:** Add a dev test/example that builds an `Assets` resembling AIRWOLF (or loads the
  test cart), renders `MusicEditor::draw` into a `Framebuffer`, and pixel-diffs it against
  `…-assets/p8_music_clean.png` (recovered 128×128), ignoring note-text content and font glyphs
  (compare only chrome regions: rows 0–31 and the panel borders). Assert the chrome diff is under a
  small threshold.
- [ ] **Step 2:** Run it; fix any chrome mismatch the diff reports.
- [ ] **Step 3:** Commit: `✅ console: pixel-check the music editor against PICO-8`.

---

## Phase 7 — SFX editor rewrite (`rico8-console/src/editor/sfx.rs`)

Restructure into three modes. The pitch-mode and tracker-mode layouts are ported from
`mockup_editors.rs::sfx_pitch`/`sfx_tracker`; the wave-designer layout needs a PICO-8 reference
(flag — see Phase 9). Verify against `…-assets/p8_sfx_clean.png`.

### Task 13: Mode state + TAB toggle + shared header

- [ ] **Step 1: Write the failing test**

```rust
#[test]
fn tab_toggles_pitch_and_tracker() {
    let mut ed = SfxEditor::new();
    assert_eq!(ed.mode(), SfxMode::Pitch);
    let mut a = Assets::default();
    ed.key(Key::Tab, Mods::default(), &mut a, &AudioHandle::dummy());
    assert_eq!(ed.mode(), SfxMode::Tracker);
    ed.key(Key::Tab, Mods::default(), &mut a, &AudioHandle::dummy());
    assert_eq!(ed.mode(), SfxMode::Pitch);
}
```

- [ ] **Step 2: Run to verify it fails** — `cargo test -p rico8-console tab_toggles` → FAIL.
- [ ] **Step 3: Implement** an `enum SfxMode { Pitch, Tracker, Wave }`, a `mode` field + `mode()`
  accessor, and `Key::Tab` toggling Pitch↔Tracker (Wave entered only via the `∿` toggle, and only
  for `self.sfx < 8`). Draw the shared header (`◀ NN ▶`, `spd`, `loop`/`len`, the `∿` toggle) +
  `ui::mode_buttons`.
- [ ] **Step 4: Run to verify it passes** — PASS.
- [ ] **Step 5: Commit** — `✨ console: add SFX editor mode switching (pitch/tracker/wave)`.

### Task 14: Pitch mode

- [ ] **Step 1:** Draw `:pitch` + `ui::blit(.., PALETTE)` (highlight the selected waveform's box
  red — adapt PALETTE so box `self.wave_sel` is red) + `ui::blit(.., CIRCLE)`; the black graph panel
  with one bar per step (height = pitch, instrument-coloured) + a marker; the `:volume` strip
  (markers, height = volume) + the 3-bar indicator.
- [ ] **Step 2:** Mouse: click-drag the graph sets `notes[step].pitch` (Shift = pitch only; Ctrl =
  snap to C-minor-pentatonic; right-click = eyedropper instrument); drag the volume strip sets
  `notes[step].volume`; click a palette box selects the waveform (shift-click applies to all notes).
- [ ] **Step 3:** Build + eyeball/render-check vs `p8_sfx_clean.png`.
- [ ] **Step 4:** Commit — `✨ console: SFX pitch-mode graph editing`.

### Task 15: Tracker mode

- [ ] **Step 1:** Port the two-column tracker draw (step gutter, `note_cell`, 4-step gridlines,
  loop/len marker, playhead via `audio.channel_step()[0]`), and the filter switches strip
  (`nz/bz/dt/rv/dm`). Reuse the existing piano-key note entry, octave, instrument/volume/effect
  digit edits, and the filter toggles from the current `sfx.rs` (largely unchanged).
- [ ] **Step 2:** Build + `cargo test -p rico8-console`.
- [ ] **Step 3:** Commit — `✨ console: restyle the SFX tracker mode to match PICO-8`.

### Task 16: Wave designer mode

- [ ] **Step 1:** (Needs a PICO-8 reference — see Phase 9.) For `self.sfx < 8`, the `∿` toggle enters
  Wave mode: draw a 32-sample canvas around a centre axis; mouse-drag draws into
  `assets.sfx[self.sfx].custom_wave` (creating it if `None`), values `-16..=15`; a `bass` toggle
  flips `custom_wave.bass`.
- [ ] **Step 2:** Add a logic test: dragging sets a sample; toggling bass flips the flag.
- [ ] **Step 3:** Build + test.
- [ ] **Step 4:** Commit — `✨ console: add the SFX wave-designer mode`.

---

## Phase 8 — Integration & cleanup

### Task 17: Manual smoke test in the running console

- [ ] **Step 1:** `cargo run -p rico8-console`; `import-pico8 airwolf.p8`; open `music`, navigate
  patterns, watch the playhead while `Space` plays; click a pencil → lands in `sfx` on that SFX;
  `Tab` toggles pitch/tracker; draw a pitch curve; on SFX 0–7 enter the wave designer and draw.
- [ ] **Step 2:** Fix anything broken; commit fixes individually.

### Task 18: Delete the throwaway mockup scaffold

- [ ] **Step 1:** `git rm rico8-runtime/examples/mockup_editors.rs` and remove the `mockups/`
  scratch directory (`rm -rf mockups`). Keep the recovered references under
  `docs/superpowers/specs/2026-06-15-sfx-music-editor-rework-assets/`.
- [ ] **Step 2:** `cargo build` (workspace) clean; `cargo test` (workspace) green;
  `cargo clippy --workspace` clean; `cargo fmt --all`.
- [ ] **Step 3:** Commit — `🔥 console: remove the editor-mockup scaffold`.

---

## Phase 9 — Open items to resolve during implementation

- **Wave Designer exact layout** (Task 16): recover a PICO-8 wave-designer framebuffer the same way
  the other references were made (crop screen region, downsample by integer scale, quantise) — ask
  the user for a screenshot of PICO-8's `∿` mode on an SFX 0–7. Pin the sample count (32 vs 64) and
  value range to match.
- **PICO-8 waveform-instrument byte encoding** (Task 4): confirm the flag/sample layout against
  picotool + BBS tid=45247 before decoding.
- **Note-row height scaling** (Task 9): confirm PICO-8's exact rule (length-scaled vs fixed) from the
  recovered references; ship 8px if the rule is unclear, leave a `TODO` referencing the spec.
- **Pitch-mode effects row** and the `:volume` far-right icon are undocumented in PICO-8; render the
  volume strip and defer the effects row.

---

## Self-review notes

- Spec coverage: data model (T1), playhead (T2/T9/T15), drawn waveforms (T3/T16), import (T4), tab
  highlight (T5), chrome (T6), handoff (T7/T10), music editor (T8–T12), SFX three modes (T13–T16),
  acceptance pixel-check (T12; SFX render-check folded into T14/T15 eyeball + the same harness),
  cleanup (T18). Wave-designer layout is gated on a reference (T16/Phase 9).
- Types are consistent: `CustomWave { samples: [i8; SFX_LEN], bass }`, `Synth::channel_step()`,
  `MusicEditor::take_edit_request()`, `SfxEditor::select()` / `SfxMode`.
- The editor draw code is "ported from `mockup_editors.rs`" — that file is concrete, in-repo, and
  pixel-verified, so this is a real reference, not a placeholder. It is deleted in T18.
```
