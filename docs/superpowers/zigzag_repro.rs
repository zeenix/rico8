// Standalone reproduction for the diagonal "zigzag"/staircase motion artifact.
// Run with:  rustc -O docs/superpowers/zigzag_repro.rs -o /tmp/zigzag && /tmp/zigzag
//
// Reproduces the RICO-8 motion+draw model: accumulate velocity per frame,
// floor() at draw. Proves: (1) f32 vs f64 vs PICO-8 16.16 are equivalent,
// (2) the zigzag is about heading AND axis phase, not precision,
// (3) a perfect 45deg equal-speed diagonal zigzags once x/y fractions differ.
//
// Step classification per frame transition:
//   D = both axes move (clean diagonal)   H = x only
//   V = y only                            . = neither
// A run of H/V (or irregular D) is the zigzag; a run of pure D/. is clean.

fn steps(p: &[(i32, i32)]) -> String {
    let mut s = String::new();
    for w in p.windows(2) {
        let (dx, dy) = (w[1].0 - w[0].0, w[1].1 - w[0].1);
        s.push(match (dx != 0, dy != 0) {
            (true, true) => 'D',
            (true, false) => 'H',
            (false, true) => 'V',
            (false, false) => '.',
        });
    }
    s
}

fn f32_run(x0: f32, y0: f32, vx: f32, vy: f32, n: usize) -> Vec<(i32, i32)> {
    let (mut x, mut y) = (x0, y0);
    let mut o = Vec::new();
    for _ in 0..n {
        o.push((x.floor() as i32, y.floor() as i32));
        x += vx;
        y += vy;
    }
    o
}
fn f64_run(x0: f64, y0: f64, vx: f64, vy: f64, n: usize) -> Vec<(i32, i32)> {
    let (mut x, mut y) = (x0, y0);
    let mut o = Vec::new();
    for _ in 0..n {
        o.push((x.floor() as i32, y.floor() as i32));
        x += vx;
        y += vy;
    }
    o
}
// PICO-8 numbers are signed 16.16 fixed point. floor() = arithmetic >> 16.
fn fixed_run(x0: f64, y0: f64, vx: f64, vy: f64, n: usize) -> Vec<(i32, i32)> {
    let fx = |v: f64| (v * 65536.0).round() as i64;
    let (mut x, mut y, vx, vy) = (fx(x0), fx(y0), fx(vx), fx(vy));
    let mut o = Vec::new();
    for _ in 0..n {
        o.push(((x >> 16) as i32, (y >> 16) as i32));
        x += vx;
        y += vy;
    }
    o
}

fn main() {
    let n = 32;

    println!("== PART 1: f32 vs f64 vs 16.16, same nominal motion ==");
    println!("(equal speed, x0=y0 so phases are locked)\n");
    for &(label, v) in &[("1.0 exact", 1.0), ("0.5 exact", 0.5), ("0.7 inexact", 0.7)] {
        let a = f32_run(0.0, 0.0, v as f32, v as f32, n);
        let b = f64_run(0.0, 0.0, v, v, n);
        let c = fixed_run(0.0, 0.0, v, v, n);
        let diff = a.iter().zip(&b).filter(|(p, q)| p != q).count();
        println!("v={label}");
        println!("  f32  : {}", steps(&a));
        println!("  f64  : {}", steps(&b));
        println!("  16.16: {}", steps(&c));
        println!("  f32-vs-f64 differing positions: {diff}/{n}\n");
    }

    println!("== PART 2: same speed (0.7), different HEADING ==");
    println!("(off-45 makes |vx| != |vy| -> H/V zigzag, identical across types)\n");
    for deg in [45.0_f64, 30.0, 10.0] {
        let (vx, vy) = (0.7 * deg.to_radians().cos(), 0.7 * deg.to_radians().sin());
        println!("{deg}deg");
        println!("  f32  : {}", steps(&f32_run(0.0, 0.0, vx as f32, vy as f32, n)));
        println!("  f64  : {}", steps(&f64_run(0.0, 0.0, vx, vy, n)));
        println!("  16.16: {}", steps(&fixed_run(0.0, 0.0, vx, vy, n)));
        println!();
    }

    println!("== PART 3: perfect 45deg, EQUAL speed, different x/y PHASE ==");
    println!("(this is the Right+Up button case; zigzags once fractions differ)\n");
    for &(v, y0) in &[
        (1.0_f32, 10.5_f32),
        (0.5, 10.5),
        (0.7, 10.3),
        (1.0 / 2.0_f32.sqrt(), 10.5),
    ] {
        println!("speed {v:.4}");
        println!("  same phase x0=10.0 y0=10.0 : {}", steps(&f32_run(10.0, 10.0, v, v, n)));
        println!("  diff phase x0=10.0 y0={y0:<4} : {}", steps(&f32_run(10.0, y0, v, v, n)));
        println!();
    }
}
