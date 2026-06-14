# Reproducing the cart limits

This is a hands-on guide for deliberately tripping each of RICO-8's three cart
limits and seeing exactly what shows up, in every UI that can observe it. It is
meant for developers and testers verifying that the limits actually fire. For
the author-facing explanation of *why* the limits exist, see
[docs/LIMITS.md](LIMITS.md).

All three limits are built around one number: **128 K**.

| limit          | value   | what you see                                  | build | run |
| -------------- | ------- | --------------------------------------------- | ----- | --- |
| cart size      | 128 KiB | build warning; `export` rejected              | yes   | no  |
| RAM            | 128 KiB | "ran out of memory" error screen              | no    | yes |
| per-frame work | 128 K   | "ran too long (infinite loop?)" error screen  | no    | yes |

"build" limits are observed where carts are *compiled and packed* — the desktop
console and the headless CLI (both are the `rico8` binary). "run" limits are
observed where carts are *played* — the desktop console, the web player, and
the SDL player.

Which UI observes which limit:

| UI              | cart size | RAM | per-frame work |
| --------------- | --------- | --- | -------------- |
| desktop console | yes       | yes | yes            |
| headless CLI    | yes       | no  | no             |
| web player      | no        | yes | yes            |
| SDL player      | no        | yes | yes            |

The headless CLI builds and exports but never runs a cart, so it never shows the
RAM or per-frame-work error screens. The web and SDL players run a packed cart
but never build one, so they never show the cart-size warning or rejection.

---

## The reproduction carts

The two runtime limits (RAM and per-frame work) are tripped with the
[`examples/stress`](../examples/stress) cart, which is purpose-built for this.
Its controls (from `examples/stress/src/lib.rs`):

- **Up / Down** — raise or lower the workload level `N`.
- **O** — toggle the memory probe (holds `N * 4 KiB` on the heap).
- **X** — toggle the compute probe (runs `N * 2000` arithmetic iterations).
- Drawing `N * 20` shapes is always on, as a wall-clock baseline.

A trap ends the cart, so the probes are independently toggleable: enable **one**
probe, then ramp `N` up with Up until that limit trips. The HUD spells out
`MEM ON`/`MEM OFF` and `CPU ON`/`CPU OFF` (green when on, grey when off) so you
can see which probe is live.

The cart-size limit has no example that trips it — every shipped cart is a few
kilobytes. The recipe below bloats a scratch project past 128 KiB on purpose.

---

## Per-frame work — 128 K

A single `update()` or `draw()` call that loops too long, or simply does too
much, is stopped instead of freezing the player. The runtime emits, for the
phase that overran:

```text
update() ran too long
(infinite loop?)
```

(or `draw() ran too long` if the draw call is the one that overran).

### Trip it

Enable only the compute probe, then ramp `N`:

1. Boot the cart in any runner (see the per-UI commands below).
2. Press **X** once so the HUD reads `CPU ON`; leave the memory probe at
   `MEM OFF`.
3. Tap **Up** repeatedly to raise `N`. Each step adds `N * 2000` iterations to
   the frame. At a high enough `N` the frame overruns the per-frame work budget
   and the "ran too long" error screen appears.

### What you see, per UI

**Desktop console.** The run stops and you drop back to the console, which
prints, in the scrollback:

```text
** error in update **
update() ran too long
(infinite loop?)
```

The header is red and the message lines are orange. Press `Esc` / type as usual
to carry on.

**Web player.** The canvas switches to the shared error screen: a red top bar
reading `rico-8`, the heading `** runtime error **`, and the message:

```text
runtime error in update:
update() ran too long
(infinite loop?)
```

with a `press f5 to restart` hint at the bottom. Pressing **F5** reloads the
page and reboots the cart.

**SDL player.** Same shared error screen (red bar, `** runtime error **`, the
`runtime error in update:` text), with a `hold o+x to exit` hint at the bottom;
hold both action buttons to return to the picker. The same line is also written
to stderr as `rico8-player: runtime error: …`.

---

## RAM — 128 K

A running cart may use at most 128 KiB of total RAM. Overrunning it stops the
cart with, for the phase that overran:

```text
update() ran out of memory
(128K limit)
```

### Trip it

Enable only the memory probe, then ramp `N`:

1. Boot the cart in any runner.
2. Press **O** once so the HUD reads `MEM ON`; leave the compute probe at
   `CPU OFF`.
3. Tap **Up** repeatedly to raise `N`. Each step asks the cart to hold another
   `N * 4 KiB` on the heap. At a high enough `N` the allocation pushes past the
   128 K budget and the "ran out of memory" error screen appears.

### What you see, per UI

**Desktop console.** Back in the console scrollback:

```text
** error in update **
update() ran out of memory
(128K limit)
```

(red header, orange message).

**Web player.** The shared error screen on the canvas:

```text
runtime error in update:
update() ran out of memory
(128K limit)
```

with the `press f5 to restart` hint.

**SDL player.** The same shared error screen with the `hold o+x to exit` hint,
and `rico8-player: runtime error: …` on stderr.

---

## Cart size — 128 KiB

The compiled game wasm must be no larger than 128 KiB. Building an over-size
cart still succeeds but prints a non-fatal warning; **`export` is rejected**
outright. This limit is observed only in the console and CLI, which build and
pack carts; the players never build, so they never see it.

There is no example cart that trips this — all shipped carts are tiny. The
recipe below deliberately bloats one. The approach is general: take a project,
pull in a heavy dependency that pulls in a lot of code, and rebuild. The example
here uses `regex`, which compiles to a large wasm module.

### Bloat recipe (tested)

This was run end to end and produced the messages quoted below.

The `rico8` command below is the console binary. In a checkout there is no
installed `rico8` yet, so either build/install it once or run it via `cargo run
--release -p rico8-console -- <args>` (e.g. `cargo run --release -p rico8-console
-- new bloat`); the bare `rico8 …` commands are shorthand for that.

1. Create a scratch project:

   ```text
   rico8 new bloat
   ```

2. Edit `bloat/Cargo.toml` to depend on `rico8` with its default features (so
   the cart has a heap) and add a heavy dependency:

   ```toml
   [dependencies]
   rico8 = { path = "…/rico8" }
   regex = "1"
   ```

   (Drop the `default-features = false` from the scaffolded `rico8` line; the
   default-features build brings in a heap, which `regex` needs.)

3. Edit `bloat/src/lib.rs` to actually *use* the dependency, so it is not
   stripped as dead code — e.g. remove the `#![no_std]` line and compile and
   run a `regex::Regex` inside `update()`.

4. Build it:

   ```text
   rico8 build bloat
   ```

   The build succeeds but warns (this is the exact format from the builder;
   the byte count depends on the dependency):

   ```text
   warning: cart wasm is 1015925 bytes; over the 128K limit (131072)
   ```

   In the desktop console the same warning appears in orange in the scrollback
   after `build ok`. From the headless CLI (`rico8 build …`) it is printed to
   stderr.

5. Try to export it:

   ```text
   rico8 export bloat bloat.png
   ```

   The export is rejected. The CLI exits non-zero and prints to stderr:

   ```text
   Error: cart wasm is 1015925 bytes; the limit is 131072 (128K)
   ```

   No PNG is written. (The `Error:` prefix is the CLI's; the rest is the
   verbatim rejection message.) In the desktop console, the same
   `cart wasm is … bytes; the limit is 131072 (128K)` text is shown in the
   scrollback and no cart file is produced.

In the run above, the baseline `rico8 new` project compiled to about 1 KiB of
wasm, and adding `regex` took it to roughly 992 KiB — comfortably over the
128 KiB limit — so the warning and the export rejection both fired.

---

## The run commands

The runtime limits (RAM, per-frame work) need a *running* cart. Here is how to
get `examples/stress` running in each player.

### Desktop console

From the repo root, boot the console with the project loaded, then build and run
it from the console prompt:

```text
cargo run --release -p rico8-console -- examples/stress
```

At the `>` prompt type `run` (compiles to wasm and runs; `Esc` returns to the
console). In the running cart, arrow keys are the d-pad and `Z`/`X` are the O/X
buttons, so use **Up**/**Down** for `N` and **Z** (O) / **X** (X) to toggle the
probes.

### Web player

The web player runs a packed cart, so first export `examples/stress` to a
self-contained HTML page, then open it in a browser:

```text
cargo run --release -p rico8-console -- export-web examples/stress stress.html
```

Open `stress.html` and click to boot. Keyboard mapping matches the console
(arrows + `Z`/`X`); on a touch screen the on-page d-pad and O/X buttons work
too.

### SDL player

The SDL player also runs a packed cart. Export `examples/stress` to a PNG cart
first, then point the player at it:

```text
cargo run --release -p rico8-console -- export examples/stress stress.png
cargo run --release -p rico8-player -- stress.png
```

On the device the d-pad moves and the two action buttons are O / X; with a
keyboard, arrows + `Z`/`X` work the same way.
