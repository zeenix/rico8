//! Coherent sub-pixel motion.
//!
//! RICO-8 floors every position to a pixel at draw time, and a cart's `x` and
//! `y` are independent variables. When something moves diagonally at less than
//! one pixel per frame, `floor(x)` and `floor(y)` tick on different frames
//! unless their fractional phases happen to line up — so the steps alternate
//! "x only" then "y only" and the sprite *zigzags* along the diagonal instead
//! of climbing a clean staircase. This is pure integer-grid geometry (it is not
//! a floating-point precision problem, and PICO-8 has it too); the only diagonal
//! the four-button d-pad can produce — holding two buttons for a 45° heading —
//! hits it whenever the two axes start on different sub-pixel fractions.
//!
//! [`Body`] fixes it for carts that opt in. It owns the trajectory (which the
//! immediate-mode renderer can't see) and emits a *phase-coherent* render
//! position: the faster axis steps freely, and a slower-axis step is held back
//! to land on the same frame as a faster-axis step, merging the two into a
//! single diagonal move. The result is a regular staircase. Game logic keeps
//! reading the exact sub-pixel position; only the drawn pixel is made coherent.
//!
//! ```no_run
//! use rico8::*;
//!
//! struct Mob { body: Body }
//!
//! impl Game for Mob {
//!     fn update(&mut self, ctx: &mut Context) {
//!         // Body just needs a per-frame movement; compute it however the game
//!         // likes — input, physics, a chase. Here it's the held d-pad at a
//!         // sub-pixel speed (the case that would otherwise zigzag).
//!         let speed = 0.6;
//!         let held = ctx.buttons_down();
//!         let mut dx = 0.0;
//!         let mut dy = 0.0;
//!         if held.contains(Button::Left) { dx -= speed; }
//!         if held.contains(Button::Right) { dx += speed; }
//!         if held.contains(Button::Up) { dy -= speed; }
//!         if held.contains(Button::Down) { dy += speed; }
//!         self.body.move_by(dx, dy);
//!     }
//!
//!     fn draw(&self, gfx: &mut Graphics) {
//!         gfx.clear(Color::BLACK);
//!         // draw_x/draw_y are the coherent pixel; collision uses x()/y().
//!         gfx.sprite(SpriteId(1), self.body.draw_x(), self.body.draw_y());
//!     }
//! }
//! ```

/// `|v|`, branch form. `f32::abs` is not guaranteed outside `std`, and the SDK
/// builds `no_std`; this needs no math library.
fn fabs(v: f32) -> f32 {
    if v < 0.0 {
        -v
    } else {
        v
    }
}

/// `floor(v)` as an `i32`, matching what the console does at draw time, without
/// the `std`-only `f32::floor`. Positions on a 128x128 screen are tiny, so the
/// saturating float-to-int cast never bites.
fn floor_i32(v: f32) -> i32 {
    let t = v as i32;
    if (t as f32) > v {
        t - 1
    } else {
        t
    }
}

/// A moving body that renders a clean pixel staircase instead of a zigzag.
///
/// Hold the full-precision position for game logic, advance it each frame with
/// [`move_by`](Body::move_by), and draw at [`draw_x`](Body::draw_x) /
/// [`draw_y`](Body::draw_y).
///
/// The drawn pixel never differs from `floor(`[`x`](Body::x)`)` /
/// `floor(`[`y`](Body::y)`)` by more than one pixel, and that difference does
/// not accumulate — so collision against the exact position stays honest while
/// the sprite stops shimmering.
#[derive(Debug, Clone, Copy)]
pub struct Body {
    /// Full-precision position — the truth for game logic and collision.
    x: f32,
    y: f32,
    /// Coherent render position — what to draw, floored and phase-aligned.
    rx: i32,
    ry: i32,
}

impl Body {
    /// A body at a starting position. The drawn pixel starts floored, as usual.
    pub fn new(x: f32, y: f32) -> Self {
        Self {
            x,
            y,
            rx: floor_i32(x),
            ry: floor_i32(y),
        }
    }

    /// Advance one frame by a per-frame delta (i.e. velocity in px/frame).
    ///
    /// The cart owns its own acceleration, friction and input handling; this
    /// just takes the resulting movement for the frame and updates both the
    /// exact position and the coherent render pixel.
    pub fn move_by(&mut self, dx: f32, dy: f32) {
        self.x += dx;
        self.y += dy;
        let fx = floor_i32(self.x);
        let fy = floor_i32(self.y);

        // Whole-pixel-or-faster motion on either axis can't produce the
        // sub-pixel phase zigzag (that axis steps every frame), and we want the
        // drawn pixel exact there, so snap straight to the floored position.
        let (ax, ay) = (fabs(dx), fabs(dy));
        if ax >= 1.0 || ay >= 1.0 {
            self.rx = fx;
            self.ry = fy;
            return;
        }

        // Sub-pixel motion. The faster ("major") axis renders at floor(true)
        // exactly. A step on the slower ("minor") axis is deferred until the
        // frame the major axis also steps, so they land together as one
        // diagonal move rather than as a lone horizontal then a lone vertical —
        // the alternation that reads as a zigzag. The `>= 2` guard forces the
        // minor axis to catch up if it ever lags two pixels, bounding the
        // render position to within one pixel of the true position forever.
        if ax >= ay {
            let major_stepped = fx != self.rx;
            self.rx = fx;
            let lag = fy - self.ry;
            if lag != 0 && (major_stepped || lag.abs() >= 2) {
                self.ry += lag.signum();
            }
        } else {
            let major_stepped = fy != self.ry;
            self.ry = fy;
            let lag = fx - self.rx;
            if lag != 0 && (major_stepped || lag.abs() >= 2) {
                self.rx += lag.signum();
            }
        }
    }

    /// The exact sub-pixel x — use this for game logic and collision.
    pub fn x(&self) -> f32 {
        self.x
    }

    /// The exact sub-pixel y — use this for game logic and collision.
    pub fn y(&self) -> f32 {
        self.y
    }

    /// The exact sub-pixel position `(x, y)`.
    pub fn pos(&self) -> (f32, f32) {
        (self.x, self.y)
    }

    /// The coherent x pixel to draw at.
    pub fn draw_x(&self) -> i32 {
        self.rx
    }

    /// The coherent y pixel to draw at.
    pub fn draw_y(&self) -> i32 {
        self.ry
    }

    /// The coherent render position `(draw_x, draw_y)`.
    pub fn draw_pos(&self) -> (i32, i32) {
        (self.rx, self.ry)
    }

    /// Teleport to a position, re-snapping the render pixel (no coherent step —
    /// this is a jump, not motion).
    pub fn set_pos(&mut self, x: f32, y: f32) {
        self.x = x;
        self.y = y;
        self.rx = floor_i32(x);
        self.ry = floor_i32(y);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Per-frame step classification of a render path, like the repro script:
    /// `D` both axes, `H` x only, `V` y only, `.` neither. A lone `V` wedged
    /// between `H`s (or vice versa) is the zigzag.
    fn classify(path: &[(i32, i32)]) -> String {
        path.windows(2)
            .map(|w| {
                let (dx, dy) = (w[1].0 - w[0].0, w[1].1 - w[0].1);
                match (dx != 0, dy != 0) {
                    (true, true) => 'D',
                    (true, false) => 'H',
                    (false, true) => 'V',
                    (false, false) => '.',
                }
            })
            .collect()
    }

    /// Drive a `Body` at constant velocity and collect the render path.
    fn body_path(x0: f32, y0: f32, vx: f32, vy: f32, n: usize) -> Vec<(i32, i32)> {
        let mut b = Body::new(x0, y0);
        let mut path = Vec::with_capacity(n);
        for _ in 0..n {
            path.push((b.draw_x(), b.draw_y()));
            b.move_by(vx, vy);
        }
        path
    }

    /// The naive model: independent per-axis accumulation, floored at draw.
    fn naive_path(x0: f32, y0: f32, vx: f32, vy: f32, n: usize) -> Vec<(i32, i32)> {
        let (mut x, mut y) = (x0, y0);
        let mut path = Vec::with_capacity(n);
        for _ in 0..n {
            path.push((floor_i32(x), floor_i32(y)));
            x += vx;
            y += vy;
        }
        path
    }

    /// The defining "no zigzag" property: the minor (slower) axis never steps
    /// on a frame the major (faster) axis does not. Equal speed counts x as
    /// major, matching `Body`.
    fn minor_never_steps_alone(path: &[(i32, i32)], vx: f32, vy: f32) -> bool {
        let x_major = fabs(vx) >= fabs(vy);
        path.windows(2).all(|w| {
            let (dx, dy) = (w[1].0 - w[0].0, w[1].1 - w[0].1);
            let (major, minor) = if x_major { (dx, dy) } else { (dy, dx) };
            minor == 0 || major != 0
        })
    }

    #[test]
    fn button_diagonal_is_a_clean_staircase_at_any_phase() {
        // The Right+Up case: a perfect 45° equal-speed heading. Naively this
        // zigzags whenever x and y start on different fractions; Body does not.
        for &(speed, y0) in &[(0.5_f32, 10.5_f32), (0.7, 10.3), (0.6, 10.91)] {
            let naive = classify(&naive_path(10.0, y0, speed, speed, 40));
            let body = classify(&body_path(10.0, y0, speed, speed, 40));
            assert!(
                naive.contains('V') && naive.contains('H'),
                "test setup: naive path should zigzag, got {naive}"
            );
            assert!(
                !body.contains('V') && !body.contains('H'),
                "Body diagonal should be pure D/.: got {body}"
            );
        }
    }

    #[test]
    fn matched_phase_diagonal_is_unchanged() {
        // When x and y already share a fraction the naive path is already clean;
        // Body must not make it worse.
        let body = classify(&body_path(10.0, 10.0, 0.5, 0.5, 32));
        assert_eq!(body, ".D.D.D.D.D.D.D.D.D.D.D.D.D.D.D.");
    }

    #[test]
    fn no_lone_minor_step_at_any_heading() {
        // Sweep headings (including off-45, which a cart could choose via
        // unequal axis speeds). Body must never emit a lone minor-axis step.
        for k in 1..18 {
            let deg = k as f32 * 5.0; // 5°..85°
            let rad = deg * core::f32::consts::PI / 180.0;
            // cos/sin via the std test build; values are just a fixed heading.
            let (vx, vy) = (0.7 * rad.cos(), 0.7 * rad.sin());
            let path = body_path(0.3, 0.6, vx, vy, 200);
            assert!(
                minor_never_steps_alone(&path, vx, vy),
                "lone minor step at {deg}°: {}",
                classify(&path)
            );
        }
    }

    #[test]
    fn render_tracks_true_position_within_one_pixel() {
        // The render pixel must stay within 1px of floor(true) for every
        // heading, and it must not drift with time.
        for k in 1..18 {
            let deg = k as f32 * 5.0;
            let rad = deg * core::f32::consts::PI / 180.0;
            let (vx, vy) = (0.7 * rad.cos(), 0.7 * rad.sin());
            let mut b = Body::new(3.3, 7.6);
            for _ in 0..50_000 {
                b.move_by(vx, vy);
                let ex = (b.draw_x() - floor_i32(b.x())).abs();
                let ey = (b.draw_y() - floor_i32(b.y())).abs();
                assert!(ex <= 1 && ey <= 1, "drift {ex},{ey} at {deg}°");
            }
        }
    }

    #[test]
    fn axis_aligned_and_fast_motion_render_exactly() {
        // Pure horizontal, pure vertical, and >=1px/frame motion have no zigzag
        // to fix, so the drawn pixel must equal floor(true) exactly.
        for &(vx, vy) in &[
            (0.5_f32, 0.0_f32),
            (0.0, -0.7),
            (1.0, 1.0),
            (1.7, 1.7),
            (2.3, -0.4),
        ] {
            let mut b = Body::new(10.0, 10.5);
            for _ in 0..300 {
                b.move_by(vx, vy);
                assert_eq!(b.draw_x(), floor_i32(b.x()));
                assert_eq!(b.draw_y(), floor_i32(b.y()));
            }
        }
    }

    #[test]
    fn direction_changes_stay_clean() {
        // Right, then Right+Up, then Up — like a player working the d-pad. No
        // segment, and no transition, may introduce a lone minor step.
        let mut b = Body::new(20.0, 20.3);
        let mut path = vec![(b.draw_x(), b.draw_y())];
        for (vx, vy, frames) in [(0.7, 0.0, 10), (0.7, -0.7, 16), (0.0, -0.7, 10)] {
            for _ in 0..frames {
                b.move_by(vx, vy);
                path.push((b.draw_x(), b.draw_y()));
            }
        }
        let s = classify(&path);
        // No orthogonal jitter anywhere across the segments or their seams...
        assert!(!zigzags(&s), "direction changes introduced a zigzag: {s}");
        // ...and the held diagonal really did produce diagonal steps.
        assert!(s.contains('D'), "expected a diagonal segment: {s}");
    }

    /// A path zigzags if an `H` and a `V` are adjacent — an orthogonal jitter
    /// rather than a monotone staircase.
    fn zigzags(s: &str) -> bool {
        let b = s.as_bytes();
        b.windows(2)
            .any(|w| (w[0] == b'H' && w[1] == b'V') || (w[0] == b'V' && w[1] == b'H'))
    }

    #[test]
    fn set_pos_teleports_and_resyncs_render() {
        let mut b = Body::new(0.0, 0.0);
        b.move_by(0.5, 0.5);
        b.set_pos(40.9, 12.2);
        assert_eq!(b.pos(), (40.9, 12.2));
        assert_eq!(b.draw_pos(), (40, 12));
    }

    #[test]
    fn negative_positions_floor_correctly() {
        let b = Body::new(-0.5, -2.0);
        assert_eq!(b.draw_pos(), (-1, -2));
    }
}
